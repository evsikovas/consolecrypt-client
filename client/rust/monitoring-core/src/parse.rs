use crate::*;
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn invalid() -> Error {
    Error::new(
        ErrorCode::InvalidResponse,
        "Invalid monitoring API response",
    )
}
fn budget() -> Error {
    Error::new(
        ErrorCode::BudgetExceeded,
        "Monitoring result exceeds configured budget",
    )
}
fn number(value: &Value) -> Result<f64, Error> {
    value
        .as_f64()
        .filter(|v| crate::valid_timestamp(*v))
        .ok_or_else(invalid)
}
fn text(value: &Value) -> Result<String, Error> {
    value.as_str().map(str::to_owned).ok_or_else(invalid)
}

fn sample(value: &Value) -> Result<Sample, Error> {
    let pair = value
        .as_array()
        .filter(|v| v.len() == 2)
        .ok_or_else(invalid)?;
    let timestamp = number(&pair[0])?;
    let value = match pair[1].as_str().ok_or_else(invalid)? {
        "NaN" => SampleValue::NaN,
        "+Inf" | "Inf" => SampleValue::PositiveInfinity,
        "-Inf" => SampleValue::NegativeInfinity,
        v => SampleValue::Finite {
            value: v
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(invalid)?,
        },
    };
    Ok(Sample { timestamp, value })
}
pub(crate) fn strings(value: Option<&Value>) -> Result<Vec<String>, Error> {
    match value {
        None => Ok(vec![]),
        Some(v) => v.as_array().ok_or_else(invalid)?.iter().map(text).collect(),
    }
}
fn data(value: &Value, b: &Budgets) -> Result<QueryData, Error> {
    let kind = value
        .get("resultType")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    let result = value.get("result").ok_or_else(invalid)?;
    match kind {
        "scalar" => Ok(QueryData::Scalar {
            sample: sample(result)?,
        }),
        "string" => {
            let pair = result
                .as_array()
                .filter(|v| v.len() == 2)
                .ok_or_else(invalid)?;
            Ok(QueryData::String {
                timestamp: number(&pair[0])?,
                value: text(&pair[1])?,
            })
        }
        "matrix" | "vector" => {
            let values = result.as_array().ok_or_else(invalid)?;
            if values.len() > b.max_series {
                return Err(budget());
            }
            let mut series = Vec::with_capacity(values.len());
            let mut total = 0usize;
            for entry in values {
                if entry.get("histogram").is_some() || entry.get("histograms").is_some() {
                    return Err(Error::new(
                        ErrorCode::UnsupportedResult,
                        "Native histogram results are not supported",
                    ));
                }
                let metric = entry
                    .get("metric")
                    .and_then(Value::as_object)
                    .ok_or_else(invalid)?;
                if metric.len() > 64 {
                    return Err(budget());
                }
                let mut labels = BTreeMap::new();
                for (key, value) in metric {
                    let value = text(value)?;
                    if key.len() > 1024 || value.len() > 4096 {
                        return Err(budget());
                    }
                    labels.insert(key.clone(), value);
                }
                let samples = if kind == "matrix" {
                    let points = entry
                        .get("values")
                        .and_then(Value::as_array)
                        .ok_or_else(invalid)?;
                    if points.len() > b.max_points_per_series {
                        return Err(budget());
                    }
                    points.iter().map(sample).collect::<Result<Vec<_>, _>>()?
                } else {
                    vec![sample(entry.get("value").ok_or_else(invalid)?)?]
                };
                total += samples.len();
                if total > b.max_total_points {
                    return Err(budget());
                }
                if samples
                    .windows(2)
                    .any(|pair| pair[0].timestamp >= pair[1].timestamp)
                {
                    return Err(invalid());
                }
                series.push(Series { labels, samples });
            }
            if kind == "matrix" {
                Ok(QueryData::Matrix { series })
            } else {
                Ok(QueryData::Vector { series })
            }
        }
        _ => Err(Error::new(
            ErrorCode::UnsupportedResult,
            "Unsupported monitoring result type",
        )),
    }
}
pub(crate) fn envelope(
    value: Value,
    http_status: u16,
    b: &Budgets,
    step: Option<f64>,
) -> Result<QueryOutcome, Error> {
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    if !matches!(status, "success" | "error")
        || (status == "success" && !(200..300).contains(&http_status))
    {
        return Err(invalid());
    }
    let warnings = strings(value.get("warnings"))?;
    let infos = strings(value.get("infos"))?;
    let data = value
        .get("data")
        .filter(|v| !v.is_null())
        .map(|v| data(v, b))
        .transpose()?;
    let error = if status == "error" {
        Some(ApiError {
            r#type: text(value.get("errorType").ok_or_else(invalid)?)?,
            message: text(value.get("error").ok_or_else(invalid)?)?,
        })
    } else {
        if data.is_none() {
            return Err(invalid());
        }
        None
    };
    let partial = !warnings.is_empty() || (error.is_some() && data.is_some());
    Ok(QueryOutcome {
        status: status.into(),
        http_status,
        data,
        warnings,
        infos,
        partial,
        error,
        effective_step_seconds: step,
    })
}
