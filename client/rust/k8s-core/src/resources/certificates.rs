use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
#[derive(Debug, Clone, Serialize)]
pub struct CertificateWarning {
    pub namespace: Option<String>,
    pub name: String,
    pub code: String,
    pub expires_at: Option<i64>,
    pub days_remaining: Option<i64>,
}

impl ResourceApi {
    /// Certificate parsing happens before serialization. Neither tls.key nor
    /// the certificate itself is returned to the UI/AI diagnostics cache.
    pub async fn certificate_warnings(
        &self,
        namespace: Option<String>,
    ) -> Result<Vec<CertificateWarning>, ResourceError> {
        let kind = self.resolve("", "v1", "secrets")?;
        let mut query = ListQuery {
            namespace,
            field_selector: Some("type=kubernetes.io/tls".into()),
            limit: Some(5),
            ..ListQuery::default()
        };
        let mut result = Vec::new();
        let mut count = 0;
        let mut seen = BTreeSet::new();
        loop {
            let path = self.list_path(&kind, &query, "list")?;
            let mut page = ProtectedJson(self.get_json(&path).await?);
            let items = page.0["items"]
                .as_array_mut()
                .ok_or(ResourceError::InvalidResponse)?;
            if items.len() > 5 {
                return Err(ResourceError::TooLarge);
            }
            for secret in items {
                complete_type_metadata(secret, &kind)?;
                count += 1;
                if count > 2000 {
                    return Err(ResourceError::TooLarge);
                }
                if let Some(warning) = certificate_warning(secret, chrono::Utc::now().timestamp())?
                {
                    result.push(warning);
                }
            }
            query.continue_token = response_string(&page.0["metadata"]["continue"], 16384)?
                .filter(|value| !value.is_empty());
            if let Some(token) = &query.continue_token {
                if !seen.insert(token.clone()) {
                    return Err(ResourceError::InvalidResponse);
                }
            } else {
                break;
            }
        }
        Ok(result)
    }
}

pub fn certificate_warning(
    secret: &Value,
    now: i64,
) -> Result<Option<CertificateWarning>, ResourceError> {
    if secret["kind"] != "Secret" || secret["type"] != "kubernetes.io/tls" {
        return Ok(None);
    }
    let name =
        response_string(&secret["metadata"]["name"], 253)?.ok_or(ResourceError::InvalidResponse)?;
    let namespace = response_string(&secret["metadata"]["namespace"], 63)?;
    let expiration = leaf_expiration(secret);
    match expiration {
        Ok(expires_at) => {
            let days = expires_at.saturating_sub(now).div_euclid(86400);
            if days >= 14 {
                return Ok(None);
            }
            Ok(Some(CertificateWarning {
                namespace,
                name,
                code: if expires_at <= now {
                    "certificate_expired"
                } else {
                    "certificate_expiring"
                }
                .into(),
                expires_at: Some(expires_at),
                days_remaining: Some(days),
            }))
        }
        Err(_) => Ok(Some(CertificateWarning {
            namespace,
            name,
            code: "certificate_invalid".into(),
            expires_at: None,
            days_remaining: None,
        })),
    }
}

fn leaf_expiration(secret: &Value) -> Result<i64, ResourceError> {
    let encoded = secret["data"]["tls.crt"]
        .as_str()
        .ok_or(ResourceError::InvalidResponse)?;
    if encoded.len() > 2 * 1024 * 1024 {
        return Err(ResourceError::TooLarge);
    }
    let bytes = Zeroizing::new(
        STANDARD
            .decode(encoded)
            .map_err(|_| ResourceError::InvalidResponse)?,
    );
    let pem = pem::parse(&*bytes).map_err(|_| ResourceError::InvalidResponse)?;
    if pem.tag() != "CERTIFICATE" {
        return Err(ResourceError::InvalidResponse);
    }
    let (_, certificate) = x509_parser::parse_x509_certificate(pem.contents())
        .map_err(|_| ResourceError::InvalidResponse)?;
    Ok(certificate.validity().not_after.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secret_page_wipes_nested_values_and_keys() {
        let mut value = json!({"items":[{"data":{"tls.key":"sensitive-first"}},{"stringData":{"token":"sensitive-unvisited"}}]});
        wipe_json(&mut value);
        assert_eq!(value, json!({}));
    }
    #[test]
    fn tls_expiry_inspection_returns_metadata_without_certificate_or_private_key() {
        let certificate =
            rcgen::generate_simple_self_signed(vec!["fixture.example".into()]).unwrap();
        let pem = certificate.cert.pem();
        let (_, parsed) = x509_parser::parse_x509_certificate(certificate.cert.der()).unwrap();
        let expiration = parsed.validity().not_after.timestamp();
        let secret = json!({"apiVersion":"v1","kind":"Secret","type":"kubernetes.io/tls","metadata":{"name":"tls","namespace":"apps"},
            "data":{"tls.crt":STANDARD.encode(pem),"tls.key":"fixture-private-key"}});
        assert!(certificate_warning(&secret, expiration - 20 * 86400)
            .unwrap()
            .is_none());
        let warning = certificate_warning(&secret, expiration - 5 * 86400)
            .unwrap()
            .unwrap();
        assert_eq!(warning.days_remaining, Some(5));
        assert_eq!(warning.code, "certificate_expiring");
        assert!(!serde_json::to_string(&warning)
            .unwrap()
            .contains("fixture-private-key"));
        assert_eq!(
            certificate_warning(&secret, expiration + 1)
                .unwrap()
                .unwrap()
                .code,
            "certificate_expired"
        );
    }
}
