//! Component boundary drafts only. No fixture JSON is an approval or Node ACK.
use super::*;
fn intent()->Value {
    json!({"version":1,"profileId":"11111111-1111-4111-8111-111111111111",
        "notificationScopeId":"notice-original","associationId":"22222222-2222-4222-8222-222222222222",
        "listenIp":"127.0.0.1","listenPort":0,"destinationIp":"::1","destinationPort":22,
        "ttlSeconds":20,"idleSeconds":10})
}
#[test]
fn public_tunnel_intent_never_accepts_caller_authority_or_transport_configuration() {
    for key in ["ownerPin","certificate","binding","policy","lease","freshness","privateKey",
        "clientCertificate","relayUrl","capability","path","now","approved","joined","drainRecorded"] {
        let mut value=intent();value[key]=json!(true);
        assert!(serde_json::from_value::<Request>(value).is_err(),"{key}");
    }
}
#[test]
fn public_tunnel_numeric_loopback_and_original_limits_are_closed() {
    let request:Request=serde_json::from_value(intent()).unwrap();request.validate().unwrap();
    for bad in ["0.0.0.0","::","localhost","127.0.0.1%0","::ffff:127.0.0.1","10.0.0.1"] {
        for field in ["listenIp","destinationIp"] {
            let mut value=intent();value[field]=json!(bad);
            assert!(serde_json::from_value::<Request>(value).unwrap().validate().is_err());
        }
    }
    for (field,bad) in [("ttlSeconds",0),("ttlSeconds",61),("idleSeconds",0),("idleSeconds",31),("destinationPort",0)] {
        let mut value=intent();value[field]=json!(bad);
        assert!(serde_json::from_value::<Request>(value).unwrap().validate().is_err());
    }
    let mut value=intent();value["idleSeconds"]=json!(21);
    assert!(serde_json::from_value::<Request>(value).unwrap().validate().is_err());
}
#[test]
fn public_tunnel_schema_rejects_duplicate_fields_and_scalar_coercion() {
    let raw=serde_json::to_string(&intent()).unwrap();
    assert!(serde_json::from_str::<Request>(&raw.replacen("{","{\"version\":1,",1)).is_err());
    for (field,value) in [("version",json!(true)),("listenPort",json!("0")),("destinationPort",json!(22.0)),
        ("ttlSeconds",json!(null)),("idleSeconds",json!({"seconds":10}))] {
        let mut body=intent();body[field]=value;assert!(serde_json::from_value::<Request>(body).is_err());
    }
}

#[test]
fn public_resume_choice_is_only_an_explicit_boolean_intent_defaulting_off() {
    let missing: Request = serde_json::from_value(intent()).unwrap();
    assert!(!missing.resume_original);
    for selected in [false, true] {
        let mut body = intent(); body["resumeOriginal"] = json!(selected);
        let request: Request = serde_json::from_value(body).unwrap();
        request.validate().unwrap(); assert_eq!(request.resume_original, selected);
    }
    for bad in [json!(null), json!(1), json!("true"), json!({"approved":true})] {
        let mut body = intent(); body["resumeOriginal"] = bad;
        assert!(serde_json::from_value::<Request>(body).is_err());
    }
    let raw = serde_json::to_string(&intent()).unwrap();
    let duplicate = raw.replacen("{", "{\"resumeOriginal\":true,\"resumeOriginal\":false,", 1);
    assert!(serde_json::from_str::<Request>(&duplicate).is_err());
}
