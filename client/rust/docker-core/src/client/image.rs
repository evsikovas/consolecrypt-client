use super::DockerClient;
use crate::DockerError;
use hyper::Method;
use serde_json::Value;
use std::{net::IpAddr, time::Duration};

pub(super) fn sha256_id(id: &str) -> bool {
    id.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    })
}

fn repository(reference: &str) -> Option<String> {
    if reference.is_empty() || reference.len() > 512 || reference.starts_with("sha256:") {
        return None;
    }
    let named = match reference.split_once('@') {
        Some((name, digest)) if sha256_id(digest) => name,
        Some(_) => return None,
        None => reference,
    };
    let last = named.rsplit('/').next()?;
    let name = if let Some((_, tag)) = last.split_once(':') {
        if tag.is_empty()
            || tag.len() > 128
            || !tag
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
            || tag.starts_with(['.', '-'])
        {
            return None;
        }
        &named[..named.len() - tag.len() - 1]
    } else {
        named
    };
    let (registry, path) = match name.split_once('/') {
        Some((first, rest)) if first.contains(['.', ':']) || first == "localhost" => (first, rest),
        _ => ("docker.io", name),
    };
    let (host, port) = registry.split_once(':').unwrap_or((registry, ""));
    if registry.contains(':')
        && (port.parse::<u16>().ok()? == 0 || !port.bytes().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    fn valid_component(component: &str, path: bool) -> bool {
        let ascii = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
        !component.is_empty()
            && ascii(component.as_bytes()[0])
            && ascii(component.as_bytes()[component.len() - 1])
            && component
                .bytes()
                .all(|c| ascii(c) || c == b'-' || path && matches!(c, b'.' | b'_'))
    }
    if !host.split('.').all(|label| valid_component(label, false))
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.starts_with("127.")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified())
        || !path.split('/').all(|part| valid_component(part, true))
    {
        return None;
    }
    let hub = matches!(
        host,
        "docker.io" | "index.docker.io" | "registry-1.docker.io"
    ) && matches!(port, "" | "443");
    Some(if hub && !path.contains('/') {
        format!("docker.io/library/{path}")
    } else if hub {
        format!("docker.io/{path}")
    } else {
        format!("{registry}/{path}")
    })
}

fn candidates(image: &Value, id: &str, source: &str) -> Result<Vec<String>, DockerError> {
    if image["Id"].as_str() != Some(id) {
        return Err(DockerError::Protocol);
    }
    let Some(repo) = repository(source) else {
        return Ok(vec![]);
    };
    let pin = source.split_once('@').map(|(_, digest)| digest);
    let mut refs = Vec::new();
    for reference in image["RepoDigests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let Some((name, digest)) = reference.split_once('@') else {
            continue;
        };
        if sha256_id(digest)
            && pin.is_none_or(|p| p == digest)
            && repository(name).as_ref() == Some(&repo)
        {
            let candidate = format!("{repo}@{digest}");
            if !refs.contains(&candidate) {
                refs.push(candidate);
            }
            if refs.len() == 3 {
                break;
            }
        }
    }
    Ok(refs)
}

impl DockerClient {
    /// Inventory metadata only. Engine labels and build configuration can contain secrets.
    pub async fn images(&self) -> Result<Vec<Value>, DockerError> {
        let list = self.json(Method::GET, "/images/json?digests=true", None).await?;
        let items = list.as_array().ok_or(DockerError::Protocol)?;
        Ok(items
            .iter()
            .map(|image| {
                serde_json::json!({
                    "id": image["Id"], "tags": image["RepoTags"], "digests": image["RepoDigests"],
                    "size": image["Size"], "created": image["Created"],
                })
            })
            .collect())
    }

    /// False means Docker kept the image because something still references it.
    pub async fn remove_image(
        &self,
        image_id: &str,
        tag: Option<&str>,
    ) -> Result<bool, DockerError> {
        if !sha256_id(image_id) {
            return Err(DockerError::InvalidRequest);
        }
        let reference = if let Some(tag) = tag {
            if tag.is_empty()
                || tag.len() > 512
                || tag.split('/').any(|s| matches!(s, "" | "." | ".."))
                || !tag
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/@:_-.[]".contains(&b))
            {
                return Err(DockerError::InvalidRequest);
            }
            // The user confirmed this named reference on this engine.
            match self
                .json(Method::GET, &format!("/images/{tag}/json"), None)
                .await
            {
                Ok(value) if value["Id"].as_str() == Some(image_id) => tag,
                Ok(_) => return Err(DockerError::InvalidRequest),
                Err(DockerError::NotFound) => return Ok(true),
                Err(error) => return Err(error),
            }
        } else {
            image_id
        };
        let path = format!("/images/{reference}?force=false&noprune=true");
        match self.json(Method::DELETE, &path, None).await {
            Ok(_) | Err(DockerError::NotFound) => Ok(true),
            Err(DockerError::Api { status: 409, .. }) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Resolve the container image to an immutable pull reference. Containerd
    /// reports locally calculated RepoDigests for tags that were never pushed.
    /// Ask the same engine for the registry descriptor before suggesting one.
    pub async fn image_repo_digests(
        &self,
        image_id: &str,
        source_image: &str,
    ) -> Result<Vec<String>, DockerError> {
        if !sha256_id(image_id) {
            return Err(DockerError::InvalidRequest);
        }
        let image = self
            .json(Method::GET, &format!("/images/{image_id}/json"), None)
            .await?;
        let refs = candidates(&image, image_id, source_image)?;
        let confirmed = tokio::time::timeout(Duration::from_secs(7), async {
            for candidate in refs {
                let path = format!("/distribution/{candidate}/json");
                let Ok(result) = self.json(Method::GET, &path, None).await else {
                    continue;
                };
                let digest = candidate.split_once('@').map(|(_, d)| d);
                if result["Descriptor"]["digest"].as_str() == digest {
                    return vec![candidate];
                }
            }
            Vec::new()
        })
        .await
        .unwrap_or_default();
        Ok(confirmed)
    }
}

#[cfg(test)]
mod tests;
