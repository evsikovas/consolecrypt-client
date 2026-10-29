//! FEAT-16 prototype. Answers one question before the feature is built: does
//! Windows' webauthn.dll, used by an ordinary (non-admin) program, return a
//! stable hmac-secret / PRF value from a USB security key such as a YubiKey?
//!
//! Steps: register a credential for `consolecrypt.dev` on the external key
//! (PIN required, hmac-secret on), then ask the key twice for the outputs of
//! two salts in PRF mode, and once in raw hmac-secret mode. The report holds
//! only SHA-256 fingerprints of the outputs, never the outputs themselves.

#[cfg(not(windows))]
fn main() {
    eprintln!("fido-probe is a Windows prototype; build and run it on Windows.");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    let mut report = Report::default();
    let code = match windows_probe::run(&mut report) {
        Ok(()) => 0,
        Err(e) => {
            report.line(format!("ОШИБКА: {e}"));
            1
        }
    };
    report.save();
    println!(
        "\nОтчёт сохранён в {}. Пришлите его, пожалуйста.",
        Report::PATH
    );
    println!("Нажмите Enter, чтобы закрыть окно.");
    let _ = std::io::stdin().read_line(&mut String::new());
    std::process::exit(code);
}

#[derive(Default)]
struct Report(Vec<String>);

impl Report {
    const PATH: &'static str = "fido-probe-report.txt";

    fn line(&mut self, text: impl Into<String>) {
        let text = text.into();
        println!("{text}");
        self.0.push(text);
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    fn save(&self) {
        let _ = std::fs::write(Self::PATH, self.0.join("\r\n") + "\r\n");
    }
}

/// First 16 hex digits of SHA-256: enough to compare, useless to an attacker.
#[cfg_attr(not(windows), allow(dead_code))]
fn fingerprint(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(windows)]
mod windows_probe {
    use super::{fingerprint, Report};
    use base64::Engine;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Networking::WindowsWebServices::*;
    use windows::Win32::System::Console::GetConsoleWindow;
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    const RP_ID: &str = "consolecrypt.dev";
    const ORIGIN: &str = "https://consolecrypt.dev";
    const CREDENTIAL_FILE: &str = "fido-probe-credential.bin";
    const TIMEOUT_MS: u32 = 120_000;

    struct Versions {
        api: u32,
        make_credential: u32,
        get_assertion: u32,
    }

    fn versions() -> Result<Versions, String> {
        let api = unsafe { WebAuthNGetApiVersionNumber() };
        // Option struct versions per API version (webauthn.h).
        let make_credential = match api {
            7.. => WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_7,
            6 => WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_6,
            4 | 5 => WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_5,
            3 => WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_4,
            _ => WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_3,
        };
        let get_assertion = match api {
            7.. => WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_7,
            4..=6 => WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_6,
            _ => {
                return Err(format!(
                    "WebAuthn API версии {api}: hmac-secret появился в версии 4 (Windows 10 21H2 / Windows 11). Обновите Windows."
                ))
            }
        };
        Ok(Versions {
            api,
            make_credential,
            get_assertion,
        })
    }

    fn window() -> HWND {
        let console = unsafe { GetConsoleWindow() };
        if console.0.is_null() {
            unsafe { GetForegroundWindow() }
        } else {
            console
        }
    }

    fn random<const N: usize>() -> [u8; N] {
        let mut bytes = [0u8; N];
        getrandom::fill(&mut bytes).expect("system random");
        bytes
    }

    fn client_data(kind: &str) -> Vec<u8> {
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random::<32>());
        format!(r#"{{"type":"{kind}","challenge":"{challenge}","origin":"{ORIGIN}","crossOrigin":false}}"#)
            .into_bytes()
    }

    fn explain(e: &windows::core::Error) -> String {
        let code = e.code().0 as u32;
        let hint = match code {
            0x800704C7 => " (отменено пользователем)",
            0x80090027 => " (неверный параметр: эта версия Windows не поддерживает запрос)",
            0x80090011 | 0x80070490 => " (ключ не найден: вставлен ли тот же YubiKey?)",
            0x80070005 => " (доступ запрещён: возможно, нужны права администратора)",
            0x8009000F => " (ключ уже зарегистрирован)",
            _ => "",
        };
        format!("0x{code:08X}{hint}: {}", e.message())
    }

    fn register(v: &Versions, report: &mut Report) -> Result<Vec<u8>, String> {
        if let Ok(id) = std::fs::read(CREDENTIAL_FILE) {
            report.line(format!(
                "Ключ уже зарегистрирован раньше (файл {CREDENTIAL_FILE}), регистрацию пропускаю."
            ));
            return Ok(id);
        }
        report.line("Шаг 1. Регистрация ключа. Вставьте YubiKey, введите PIN и коснитесь ключа.");
        let rp_id = HSTRING::from(RP_ID);
        let rp_name = HSTRING::from("ConsoleCrypt");
        let rp = WEBAUTHN_RP_ENTITY_INFORMATION {
            dwVersion: WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
            pwszId: PCWSTR(rp_id.as_ptr()),
            pwszName: PCWSTR(rp_name.as_ptr()),
            pwszIcon: PCWSTR::null(),
        };
        let mut user_id = random::<16>();
        let user_name = HSTRING::from("fido-probe@consolecrypt.dev");
        let user_display = HSTRING::from("ConsoleCrypt fido-probe");
        let user = WEBAUTHN_USER_ENTITY_INFORMATION {
            dwVersion: WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
            cbId: user_id.len() as u32,
            pbId: user_id.as_mut_ptr(),
            pwszName: PCWSTR(user_name.as_ptr()),
            pwszIcon: PCWSTR::null(),
            pwszDisplayName: PCWSTR(user_display.as_ptr()),
        };
        let mut param = WEBAUTHN_COSE_CREDENTIAL_PARAMETER {
            dwVersion: WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION,
            pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
            lAlg: WEBAUTHN_COSE_ALGORITHM_ECDSA_P256_WITH_SHA256,
        };
        let params = WEBAUTHN_COSE_CREDENTIAL_PARAMETERS {
            cCredentialParameters: 1,
            pCredentialParameters: &mut param,
        };
        let mut json = client_data("webauthn.create");
        let client = WEBAUTHN_CLIENT_DATA {
            dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
            cbClientDataJSON: json.len() as u32,
            pbClientDataJSON: json.as_mut_ptr(),
            pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
        };
        let mut hmac_on = windows::core::BOOL(1);
        let mut extension = WEBAUTHN_EXTENSION {
            pwszExtensionIdentifier: WEBAUTHN_EXTENSIONS_IDENTIFIER_HMAC_SECRET,
            cbExtension: std::mem::size_of::<windows::core::BOOL>() as u32,
            pvExtension: (&mut hmac_on as *mut windows::core::BOOL).cast(),
        };
        let mut options: WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS =
            unsafe { std::mem::zeroed() };
        options.dwVersion = v.make_credential;
        options.dwTimeoutMilliseconds = TIMEOUT_MS;
        options.Extensions = WEBAUTHN_EXTENSIONS {
            cExtensions: 1,
            pExtensions: &mut extension,
        };
        options.dwAuthenticatorAttachment = WEBAUTHN_AUTHENTICATOR_ATTACHMENT_CROSS_PLATFORM;
        options.dwUserVerificationRequirement = WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED;
        options.dwAttestationConveyancePreference = WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE;
        let make = |options: &WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS| unsafe {
            WebAuthNAuthenticatorMakeCredential(
                window(),
                &rp,
                &user,
                &params,
                &client,
                Some(options),
            )
        };
        // PRF on top of hmac-secret where the API knows it; some builds
        // reject the pair, so fall back to hmac-secret alone.
        let attestation =
            if v.make_credential >= WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_6 {
                options.bEnablePrf = windows::core::BOOL(1);
                make(&options).or_else(|e| {
                    report.line(format!(
                        "  с PRF не вышло ({}), пробую без PRF.",
                        explain(&e)
                    ));
                    options.bEnablePrf = windows::core::BOOL(0);
                    make(&options)
                })
            } else {
                make(&options)
            }
            .map_err(|e| format!("регистрация не удалась, {}", explain(&e)))?;
        let id = unsafe {
            let a = &*attestation;
            let id =
                std::slice::from_raw_parts(a.pbCredentialId, a.cbCredentialId as usize).to_vec();
            let hmac = extension_flag(&a.Extensions, "hmac-secret");
            report.line(format!(
                "Регистрация прошла. hmac-secret на ключе: {}.",
                match hmac {
                    Some(true) => "включён",
                    Some(false) => "ключ ответил «нет»",
                    None => "ответа нет",
                }
            ));
            WebAuthNFreeCredentialAttestation(Some(attestation));
            id
        };
        std::fs::write(CREDENTIAL_FILE, &id)
            .map_err(|e| format!("не удалось сохранить {CREDENTIAL_FILE}: {e}"))?;
        Ok(id)
    }

    /// A BOOL extension output, e.g. hmac-secret from MakeCredential.
    unsafe fn extension_flag(extensions: &WEBAUTHN_EXTENSIONS, name: &str) -> Option<bool> {
        let list =
            std::slice::from_raw_parts(extensions.pExtensions, extensions.cExtensions as usize);
        list.iter()
            .find(|e| e.pwszExtensionIdentifier.to_string().ok().as_deref() == Some(name))
            .filter(|e| e.cbExtension as usize >= std::mem::size_of::<windows::core::BOOL>())
            .map(|e| (*(e.pvExtension as *const windows::core::BOOL)).as_bool())
    }

    /// One assertion with two salts; returns the two outputs.
    fn assert(
        v: &Versions,
        credential: &[u8],
        salts: (&[u8; 32], &[u8; 32]),
        raw: bool,
    ) -> Result<(Vec<u8>, Vec<u8>), String> {
        let rp_id = HSTRING::from(RP_ID);
        let mut id = credential.to_vec();
        let mut allowed = WEBAUTHN_CREDENTIAL_EX {
            dwVersion: WEBAUTHN_CREDENTIAL_EX_CURRENT_VERSION,
            cbId: id.len() as u32,
            pbId: id.as_mut_ptr(),
            pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
            dwTransports: 0,
        };
        let mut allowed_ptr = &mut allowed as *mut WEBAUTHN_CREDENTIAL_EX;
        let mut allow_list = WEBAUTHN_CREDENTIAL_LIST {
            cCredentials: 1,
            ppCredentials: &mut allowed_ptr,
        };
        let (mut first, mut second) = (*salts.0, *salts.1);
        let mut salt = WEBAUTHN_HMAC_SECRET_SALT {
            cbFirst: 32,
            pbFirst: first.as_mut_ptr(),
            cbSecond: 32,
            pbSecond: second.as_mut_ptr(),
        };
        let mut salt_values = WEBAUTHN_HMAC_SECRET_SALT_VALUES {
            pGlobalHmacSalt: &mut salt,
            cCredWithHmacSecretSaltList: 0,
            pCredWithHmacSecretSaltList: std::ptr::null_mut(),
        };
        let mut json = client_data("webauthn.get");
        let client = WEBAUTHN_CLIENT_DATA {
            dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
            cbClientDataJSON: json.len() as u32,
            pbClientDataJSON: json.as_mut_ptr(),
            pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
        };
        let mut options: WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS =
            unsafe { std::mem::zeroed() };
        options.dwVersion = v.get_assertion;
        options.dwTimeoutMilliseconds = TIMEOUT_MS;
        options.dwAuthenticatorAttachment = WEBAUTHN_AUTHENTICATOR_ATTACHMENT_CROSS_PLATFORM;
        options.dwUserVerificationRequirement = WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED;
        options.dwFlags = if raw {
            WEBAUTHN_AUTHENTICATOR_HMAC_SECRET_VALUES_FLAG
        } else {
            0
        };
        options.pAllowCredentialList = &mut allow_list;
        options.pHmacSecretSaltValues = &mut salt_values;
        let assertion =
            unsafe { WebAuthNAuthenticatorGetAssertion(window(), &rp_id, &client, Some(&options)) }
                .map_err(|e| explain(&e))?;
        let outputs = unsafe {
            let a = &*assertion;
            let result = if a.pHmacSecret.is_null() {
                Err("Windows не вернула hmac-secret (pHmacSecret пустой)".to_string())
            } else {
                let s = &*a.pHmacSecret;
                let first = std::slice::from_raw_parts(s.pbFirst, s.cbFirst as usize).to_vec();
                let second = if s.pbSecond.is_null() {
                    Vec::new()
                } else {
                    std::slice::from_raw_parts(s.pbSecond, s.cbSecond as usize).to_vec()
                };
                Ok((first, second))
            };
            WebAuthNFreeAssertion(assertion);
            result
        }?;
        Ok(outputs)
    }

    fn check_mode(v: &Versions, credential: &[u8], raw: bool, report: &mut Report) -> bool {
        let name = if raw { "сырой hmac-secret" } else { "PRF" };
        let salt_a = [0xA5u8; 32];
        let salt_b = [0x5Au8; 32];
        let mut runs = Vec::new();
        for attempt in 1..=2 {
            report.line(format!(
                "Режим {name}, попытка {attempt} из 2: введите PIN и коснитесь ключа."
            ));
            match assert(v, credential, (&salt_a, &salt_b), raw) {
                Ok(outputs) => runs.push(outputs),
                Err(e) => {
                    report.line(format!("  не получилось: {e}"));
                    return false;
                }
            }
        }
        let ((a1, b1), (a2, b2)) = (&runs[0], &runs[1]);
        report.line(format!(
            "  длина ответа: {} и {} байт; отпечатки соль A: {} / {}, соль B: {} / {}",
            a1.len(),
            b1.len(),
            fingerprint(a1),
            fingerprint(a2),
            fingerprint(b1),
            fingerprint(b2)
        ));
        let stable = a1 == a2 && b1 == b2 && a1.len() == 32;
        let distinct = a1 != b1;
        report.line(format!(
            "  результат {name}: {}",
            match (stable, distinct) {
                (true, true) => "ГОДИТСЯ — ответ стабилен и зависит от соли",
                (false, _) => "НЕ ГОДИТСЯ — ответы на одну соль различаются",
                (true, false) => "НЕ ГОДИТСЯ — разные соли дали одинаковый ответ",
            }
        ));
        stable && distinct
    }

    pub fn run(report: &mut Report) -> Result<(), String> {
        report.line("ConsoleCrypt fido-probe — проверка USB-ключа безопасности (FEAT-16)");
        let v = versions()?;
        report.line(format!(
            "WebAuthn API Windows: версия {}; структуры: регистрация v{}, вход v{}.",
            v.api, v.make_credential, v.get_assertion
        ));
        let credential = register(&v, report)?;
        report.line(format!("Идентификатор ключа: {} байт.", credential.len()));
        let prf = check_mode(&v, &credential, false, report);
        let raw = check_mode(&v, &credential, true, report);
        report.line(String::new());
        report.line(format!(
            "ИТОГ: PRF — {}, сырой hmac-secret — {}.",
            if prf {
                "работает"
            } else {
                "не работает"
            },
            if raw {
                "работает"
            } else {
                "не работает"
            }
        ));
        if prf || raw {
            report.line("Вход в хранилище по YubiKey на этой Windows реализуем.");
        }
        Ok(())
    }
}
