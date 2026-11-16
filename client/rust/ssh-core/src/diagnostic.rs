//! Opaque bounded capture for ONE automation connection/review/job.
//! Values are observed at the existing trusted resolution, never re-resolved,
//! serialized, logged, exported or treated as authority. Other SSH stays opt-out.
use std::sync::Mutex;
use zeroize::Zeroizing;
const MAX_VALUES:usize=512;
const MAX_TOTAL:usize=1024*1024;
const MAX_TEXT:usize=128*1024;
const MAX_WORK:usize=2_000_000;
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum DiagnosticMaskError{Retired,Capacity,InputLimit,WorkLimit}
struct State{values:Vec<Zeroizing<String>>,bytes:usize,retired:bool}
pub struct DiagnosticCapture{state:Mutex<State>}
impl std::fmt::Debug for DiagnosticCapture{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str("DiagnosticCapture([redacted])")}}
impl Default for DiagnosticCapture{fn default()->Self{Self::new()}}
impl DiagnosticCapture{
    pub fn new()->Self{Self{state:Mutex::new(State{values:Vec::new(),bytes:0,retired:false})}}
    /// Write-only trusted input. No read/raw-list API exists.
    pub fn remember(&self,secret:&str)->Result<(),DiagnosticMaskError>{
        let mut s=self.state.lock().map_err(|_|DiagnosticMaskError::Retired)?;
        if s.retired{return Err(DiagnosticMaskError::Retired)}
        if secret.is_empty(){return Ok(())}
        if s.values.iter().any(|v|v.as_str()==secret){return Ok(())}
        if secret.len()>MAX_TEXT||s.values.len()>=MAX_VALUES||s.bytes.checked_add(secret.len()).is_none_or(|n|n>MAX_TOTAL){
            // Never continue with an incomplete known-secret set.
            s.values.clear();s.bytes=0;s.retired=true;return Err(DiagnosticMaskError::Capacity)
        }
        s.bytes+=secret.len();s.values.push(Zeroizing::new(secret.to_owned()));
        s.values.sort_by_key(|v|std::cmp::Reverse(v.len()));Ok(())
    }
    pub fn clear(&self){let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());s.values.clear();s.bytes=0;s.retired=true;}
    /// Complete logical JSON text, not a transport stream. Replacement scans
    /// ORIGINAL bytes once so overlapping secrets/marker text cannot grow
    /// recursively. Caller censors the WHOLE diagnostic on any budget failure.
    pub fn redact(&self,text:&str)->Result<String,DiagnosticMaskError>{let mut work=MAX_WORK;self.redact_with_budget(text,&mut work)}
    /// Shared budget lets a whole JSON event bound CPU across all text fields.
    pub fn redact_with_budget(&self,text:&str,remaining:&mut usize)->Result<String,DiagnosticMaskError>{
        if text.len()>MAX_TEXT{return Err(DiagnosticMaskError::InputLimit)}
        let s=self.state.lock().map_err(|_|DiagnosticMaskError::Retired)?;
        if s.retired{return Err(DiagnosticMaskError::Retired)}
        let mut output=String::with_capacity(text.len());let mut at=0;
        while at<text.len(){
            let mut matched=None;
            for secret in &s.values{
                let cost=secret.len().min(text.len()-at)+1;
                *remaining=(*remaining).checked_sub(cost).ok_or(DiagnosticMaskError::WorkLimit)?;
                if text[at..].starts_with(secret.as_str()){matched=Some(secret.len());break}
            }
            if let Some(bytes)=matched{output.push_str("[REDACTED]");at+=bytes;}
            else{let ch=text[at..].chars().next().ok_or(DiagnosticMaskError::InputLimit)?;output.push(ch);at+=ch.len_utf8();}
            if output.len()>MAX_TEXT{return Err(DiagnosticMaskError::WorkLimit)}
        }
        Ok(output)
    }
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn actual_known_password_token_pem_and_marker_overlap_are_masked(){
        let mask=DiagnosticCapture::new();let pem="-----BEGIN PRIVATE KEY-----\nowned-secret-private-body\n-----END PRIVATE KEY-----";
        for value in ["password42","token\\\"42",pem,"[","]","pass"]{mask.remember(value).unwrap();}
        let out=mask.redact(&format!("password42 token\\\"42 {pem} [pass]")).unwrap();
        for value in ["password42","token\\\"42",pem,"owned-secret-private-body"]{assert!(!out.contains(value));}assert!(out.len()<150);
    }
    #[test]fn retired_or_overflowed_capture_never_returns_original_plaintext(){
        let mask=DiagnosticCapture::new();mask.remember("known").unwrap();mask.clear();assert_eq!(mask.redact("known"),Err(DiagnosticMaskError::Retired));assert_eq!(mask.remember("new"),Err(DiagnosticMaskError::Retired));
        let mask=DiagnosticCapture::new();assert!(mask.remember(&"x".repeat(MAX_TEXT+1)).is_err());assert_eq!(mask.redact("x"),Err(DiagnosticMaskError::Retired));
    }
    #[test]fn numeric_short_unicode_and_longest_match_use_original_char_boundaries(){
        let mask=DiagnosticCapture::new();for value in ["1","токен","aba","ab"]{mask.remember(value).unwrap();}
        assert_eq!(mask.redact("1 токен abaab").unwrap(),"[REDACTED] [REDACTED] [REDACTED][REDACTED]");
    }
    #[test]fn long_common_prefix_is_charged_before_comparing_bytes(){
        let mask=DiagnosticCapture::new();let prefix="a".repeat(65_535);
        for suffix in b'b'..=b'q'{mask.remember(&format!("{prefix}{}",char::from(suffix))).unwrap();}
        let mut work=65_536;
        assert_eq!(mask.redact_with_budget(&format!("{prefix}z"),&mut work),Err(DiagnosticMaskError::WorkLimit));
        assert_eq!(work,65_536,"cost denied before the first long-prefix comparison");
        let mask=DiagnosticCapture::new();let exact="s".repeat(65_536);mask.remember(&exact).unwrap();
        let mut work=1;assert_eq!(mask.redact_with_budget(&exact,&mut work),Err(DiagnosticMaskError::WorkLimit));assert_eq!(work,1);
        let mut work=65_537;assert_eq!(mask.redact_with_budget(&exact,&mut work).unwrap(),"[REDACTED]");assert_eq!(work,0);
    }

}
