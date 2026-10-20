use super::*;

#[derive(Clone, Serialize)]
pub struct ResourceSchema {
    pub schema: Value,
    pub definitions: Value,
}
redacted_debug!(ResourceSchema);

impl ResourceApi {
    pub async fn schema(&self, kind: &ResourceKind) -> Result<ResourceSchema, ResourceError> {
        self.checked_kind(kind, "get")?;
        let key = if kind.group.is_empty() {
            format!("api/{}", kind.version)
        } else {
            format!("apis/{}/{}", kind.group, kind.version)
        };
        let index = self.get_json("/openapi/v3").await?;
        let relative = index["paths"][&key]["serverRelativeURL"]
            .as_str()
            .ok_or(ResourceError::NotFound)?;
        if relative.len() > 2048 {
            return Err(ResourceError::InvalidResponse);
        }
        let uri: http::Uri = relative
            .parse()
            .map_err(|_| ResourceError::InvalidResponse)?;
        // Only the expected discovered schema and its optional content hash
        // can be followed. API-controlled cross-origin redirects are rejected.
        if uri.scheme().is_some()
            || uri.authority().is_some()
            || uri.path() != format!("/openapi/v3/{key}")
        {
            return Err(ResourceError::InvalidResponse);
        }
        if let Some(query) = uri.query() {
            let pairs = url::form_urlencoded::parse(query.as_bytes()).collect::<Vec<_>>();
            if pairs.len() != 1
                || pairs[0].0 != "hash"
                || pairs[0].1.len() > 256
                || !pairs[0]
                    .1
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
            {
                return Err(ResourceError::InvalidResponse);
            }
        }
        let document = self.get_json(relative).await?;
        let definitions = document["components"]["schemas"]
            .as_object()
            .ok_or(ResourceError::InvalidResponse)?;
        if definitions.len() > 16_384 {
            return Err(ResourceError::TooLarge);
        }
        let schema = definitions
            .values()
            .find(|schema| {
                schema["x-kubernetes-group-version-kind"]
                    .as_array()
                    .is_some_and(|versions| {
                        versions.iter().any(|version| {
                            version["group"] == kind.group
                                && version["version"] == kind.version
                                && version["kind"] == kind.kind
                        })
                    })
            })
            .ok_or(ResourceError::NotFound)?
            .clone();
        Ok(ResourceSchema {
            schema,
            definitions: Value::Object(definitions.clone()),
        })
    }

    pub async fn create(
        &self,
        reference: &ResourceRef,
        manifest: Zeroizing<String>,
        dry_run: bool,
    ) -> Result<ResourceDetail, ResourceError> {
        if manifest.len() > MAX_MANIFEST {
            return Err(ResourceError::TooLarge);
        }
        let object = ProtectedJson(
            serde_saphyr::from_str(&manifest).map_err(|_| ResourceError::InvalidRequest)?,
        );
        validate_manifest_identity(reference, &object)?;
        if object["metadata"]["resourceVersion"].is_string()
            || object["metadata"]["uid"].is_string()
        {
            return Err(ResourceError::InvalidRequest);
        }
        let path =
            self.collection_path(&reference.api, reference.namespace.as_deref(), "create")?;
        let parameters = if dry_run {
            "fieldManager=consoleCrypt&dryRun=All"
        } else {
            "fieldManager=consoleCrypt"
        };
        let mut object = ProtectedJson(
            self.json_request(
                Method::POST,
                &format!("{path}?{parameters}"),
                serde_json::to_vec(&*object).map_err(|_| ResourceError::InvalidRequest)?,
                Some("application/json"),
            )
            .await?,
        );
        redact_object(&mut object, &reference.api, ReadOptions::default());
        let summary = row_from_value((*object).clone(), &reference.api)?;
        let yaml = serde_saphyr::to_string(&*object).map_err(|_| ResourceError::InvalidResponse)?;
        Ok(ResourceDetail {
            object: object.into_inner(),
            yaml,
            containers: Vec::new(),
            summary,
        })
    }
}
