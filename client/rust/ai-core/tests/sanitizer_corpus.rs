//! Sanitizer regression corpus (CLIENT_SPEC §15, §19).
//!
//! * Positive cases: the listed secret values must not appear in the output
//!   for every listed profile, and the listed fragments (keys, commands,
//!   placeholder labels) must remain.
//! * Negative cases: text that must survive unchanged.
//! * Every output must be a fixed point (sanitizing it again changes nothing).
//!
//! All token-shaped values are generated at runtime from a deterministic PRNG
//! so that no credential-looking literal is ever committed.

use cc_ai_core::context::HostContext;
use cc_ai_core::sanitizer::{PrivacyProfile, SanitizerSession};

const ALL: &[PrivacyProfile] = &[
    PrivacyProfile::Strict,
    PrivacyProfile::Standard,
    PrivacyProfile::Local,
];
const REMOTE: &[PrivacyProfile] = &[PrivacyProfile::Strict, PrivacyProfile::Standard];
const STRICT: &[PrivacyProfile] = &[PrivacyProfile::Strict];
const NOT_STRICT: &[PrivacyProfile] = &[PrivacyProfile::Standard, PrivacyProfile::Local];

/// Deterministic pseudo-random string over an alphabet.
fn gen(alphabet: &str, n: usize, seed: u64) -> String {
    let a: Vec<char> = alphabet.chars().collect();
    let mut x = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            a[(x % a.len() as u64) as usize]
        })
        .collect()
}

const B62: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const B64: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const HEX: &str = "0123456789abcdef";
const UPPER_NUM: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn b62(n: usize, seed: u64) -> String {
    gen(B62, n, seed)
}

/// `prefix` + random body, assembled at runtime.
fn tok(prefix: &[&str], n: usize, seed: u64) -> String {
    let mut s: String = prefix.concat();
    s.push_str(&b62(n, seed));
    s
}

fn pem(label: &str, seed: u64) -> String {
    let mut s = format!("-----BEGIN {label}-----\n");
    for i in 0..5 {
        s.push_str(&gen(B64, 64, seed + i));
        s.push('\n');
    }
    s.push_str(&gen(B64, 20, seed + 99));
    s.push_str(&format!("==\n-----END {label}-----"));
    s
}

fn jwt(seed: u64) -> String {
    format!(
        "{}{}.{}{}.{}",
        "ey",
        "JhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
        "ey",
        "JzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIn0",
        gen(
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-",
            43,
            seed
        )
    )
}

struct Pos {
    name: &'static str,
    input: String,
    profiles: &'static [PrivacyProfile],
    secrets: Vec<String>,
    keep: Vec<&'static str>,
}

fn pos(
    name: &'static str,
    input: impl Into<String>,
    profiles: &'static [PrivacyProfile],
    secrets: &[&str],
    keep: &[&'static str],
) -> Pos {
    Pos {
        name,
        input: input.into(),
        profiles,
        secrets: secrets.iter().map(|s| s.to_string()).collect(),
        keep: keep.to_vec(),
    }
}

/// Like `pos` but with owned secrets (generated tokens).
fn post(
    name: &'static str,
    input: impl Into<String>,
    profiles: &'static [PrivacyProfile],
    secrets: Vec<String>,
    keep: &[&'static str],
) -> Pos {
    Pos {
        name,
        input: input.into(),
        profiles,
        secrets,
        keep: keep.to_vec(),
    }
}

fn positives() -> Vec<Pos> {
    let mut v = Vec::new();

    // ---------------- private keys / PEM ----------------
    for (i, label) in [
        "OPENSSH PRIVATE KEY",
        "RSA PRIVATE KEY",
        "EC PRIVATE KEY",
        "DSA PRIVATE KEY",
        "PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
        "PGP PRIVATE KEY BLOCK",
    ]
    .iter()
    .enumerate()
    {
        let k = pem(label, 100 + i as u64);
        let body_line = k.lines().nth(2).unwrap().to_owned();
        v.push(post(
            "pem private key",
            format!("here is my key:\n{k}\nthanks"),
            ALL,
            vec![body_line, "BEGIN".into()],
            &["<PRIVATE_KEY_1>", "here is my key:", "thanks"],
        ));
    }
    {
        let k = pem("OPENSSH PRIVATE KEY", 200);
        let truncated: String = k.lines().take(3).collect::<Vec<_>>().join("\n");
        let line = k.lines().nth(2).unwrap().to_owned();
        v.push(post(
            "truncated private key",
            truncated,
            ALL,
            vec![line],
            &["<PRIVATE_KEY_1>"],
        ));
        let json = format!("{{\"key\": \"{}\"}}", k.replace('\n', "\\n"));
        let line = k.lines().nth(1).unwrap().to_owned();
        v.push(post(
            "private key in json",
            json,
            ALL,
            vec![line],
            &["\"key\""],
        ));
        let ssh2 = format!(
            "---- BEGIN SSH2 ENCRYPTED PRIVATE KEY ----\n{}\n---- END SSH2 ENCRYPTED PRIVATE KEY ----",
            gen(B64, 70, 201)
        );
        v.push(post(
            "ssh2 private key",
            ssh2,
            ALL,
            vec![gen(B64, 70, 201)],
            &["<PRIVATE_KEY_1>"],
        ));
        let headerless = format!("{}\n{}\n", gen(B64, 64, 202), gen(B64, 64, 203));
        v.push(post(
            "headerless key body",
            headerless,
            ALL,
            vec![gen(B64, 64, 202), gen(B64, 64, 203)],
            &[],
        ));
        let putty = format!(
            "PuTTY-User-Key-File-3: ssh-ed25519\nEncryption: none\nComment: k\nPublic-Lines: 2\n{}\n{}\nPrivate-Lines: 1\n{}\nPrivate-MAC: {}",
            gen(B64, 64, 204),
            gen(B64, 20, 205),
            gen(B64, 44, 206),
            gen(HEX, 64, 207)
        );
        v.push(post(
            "putty key",
            putty,
            ALL,
            vec![gen(B64, 44, 206)],
            &["<PRIVATE_KEY_1>"],
        ));
        let cert = pem("CERTIFICATE", 208);
        let line = cert.lines().nth(3).unwrap().to_owned();
        v.push(post("pem certificate", cert, ALL, vec![line], &["<PEM_1>"]));
    }

    // ---------------- passwords ----------------
    let p = |n: &'static str, input: &str, secret: &str, keep: &[&'static str]| {
        pos(n, input, ALL, &[secret], keep)
    };
    v.push(p(
        "sshpass quoted",
        "sshpass -p 'Hunt3r!pass' ssh deploy@web1",
        "Hunt3r!pass",
        &["sshpass -p", "<PASSWORD_1>"],
    ));
    v.push(p(
        "sshpass attached",
        "sshpass -pHunt3rpass ssh web1",
        "Hunt3rpass",
        &["sshpass -p"],
    ));
    v.push(p(
        "mysql attached -p",
        "mysql -u root -pS3cretMy1 shop",
        "S3cretMy1",
        &["mysql -u root -p"],
    ));
    v.push(p(
        "mysql quoted -p",
        "mysql -u root -p'my secret pw' shop",
        "my secret pw",
        &["mysql"],
    ));
    v.push(p(
        "mysqldump --password=",
        "mysqldump --password=DumpPw_77 shop > shop.sql",
        "DumpPw_77",
        &["--password="],
    ));
    v.push(p(
        "PGPASSWORD env",
        "PGPASSWORD=PgPw9x psql -h db -U app",
        "PgPw9x",
        &["PGPASSWORD=", "psql"],
    ));
    v.push(p(
        "export PGPASSWORD quoted",
        "export PGPASSWORD=\"Pg Pw 9x\"",
        "Pg Pw 9x",
        &["export PGPASSWORD="],
    ));
    v.push(p(
        "MYSQL_PWD",
        "MYSQL_PWD=MyPwd42 mysql -u app",
        "MyPwd42",
        &["MYSQL_PWD="],
    ));
    v.push(p(
        "redis-cli -a",
        "redis-cli -h cache -a R3disPw",
        "R3disPw",
        &["redis-cli", "-a"],
    ));
    v.push(p(
        "redis-cli --pass",
        "redis-cli --pass R3disPw2 ping",
        "R3disPw2",
        &["--pass"],
    ));
    v.push(p(
        "redis AUTH prompt",
        "127.0.0.1:6379> AUTH R3disPw3",
        "R3disPw3",
        &["AUTH"],
    ));
    v.push(p(
        "redis AUTH acl",
        "AUTH default R3disPw4",
        "R3disPw4",
        &["AUTH default"],
    ));
    v.push(p(
        "mongosh -p",
        "mongosh -u admin -p M0ngoPw --authenticationDatabase admin",
        "M0ngoPw",
        &["mongosh"],
    ));
    v.push(p(
        "sqlcmd -P",
        "sqlcmd -S sql01 -U sa -P SqlPw_123",
        "SqlPw_123",
        &["sqlcmd"],
    ));
    v.push(p(
        "docker login -p",
        "docker login -u bob -p D0ckerPw registry.local",
        "D0ckerPw",
        &["docker login"],
    ));
    v.push(p(
        "curl -u",
        "curl -u bob:CurlPw1 https://api.example.com/v1",
        "CurlPw1",
        &["curl -u", ":<PASSWORD_1>"],
    ));
    v.push(p(
        "curl --user quoted",
        "curl --user 'bob:CurlPw2' https://api.example.com",
        "CurlPw2",
        &["curl --user"],
    ));
    v.push(p(
        "wget --password",
        "wget --user=bob --password=WgetPw https://files.example.com/a",
        "WgetPw",
        &["wget"],
    ));
    v.push(p(
        "smbclient -U user%pass",
        "smbclient //srv/share -U bob%SmbPw1",
        "SmbPw1",
        &["smbclient"],
    ));
    v.push(p(
        "echo | sudo -S",
        "echo 'SudoPw1' | sudo -S apt update",
        "SudoPw1",
        &["sudo -S apt update"],
    ));
    v.push(p(
        "echo | docker --password-stdin",
        "echo DockPw2 | docker login -u bob --password-stdin",
        "DockPw2",
        &["--password-stdin"],
    ));
    v.push(p(
        "chpasswd",
        "echo 'root:ChPw3' | chpasswd",
        "ChPw3",
        &["chpasswd"],
    ));
    v.push(p(
        "here-string sudo",
        "sudo -S whoami <<< 'HereStrPw'",
        "HereStrPw",
        &["sudo -S"],
    ));
    v.push(p(
        "openssl pass:",
        "openssl pkcs12 -export -in c.pem -passout pass:P12Pw",
        "P12Pw",
        &["-passout pass:"],
    ));
    v.push(p(
        "openssl enc -k",
        "openssl enc -aes-256-cbc -k EncPw9 -in a -out b",
        "EncPw9",
        &["openssl enc"],
    ));
    v.push(p(
        "useradd -p hash",
        "useradd -m -p '$6$salt$abcdefHASH' bob",
        "$6$salt$abcdefHASH",
        &["useradd"],
    ));
    v.push(p(
        "net user",
        "net user bob NetPw1! /add",
        "NetPw1!",
        &["net user bob", "/add"],
    ));
    v.push(p(
        "cmdkey /pass:",
        "cmdkey /generic:srv /user:bob /pass:CmdkeyPw",
        "CmdkeyPw",
        &["cmdkey"],
    ));
    v.push(p(
        "ConvertTo-SecureString",
        "$s = ConvertTo-SecureString 'PsPw1' -AsPlainText -Force",
        "PsPw1",
        &["ConvertTo-SecureString"],
    ));
    v.push(p(
        "PowerShell -Password",
        "New-Thing -Name x -Password 'PsPw2'",
        "PsPw2",
        &["-Password"],
    ));
    v.push(p(
        "htpasswd -b",
        "htpasswd -b /etc/nginx/.htpasswd bob HtPw1",
        "HtPw1",
        &["htpasswd"],
    ));
    v.push(p(
        "htpasswd -nb",
        "htpasswd -nb bob HtPw2",
        "HtPw2",
        &["htpasswd -nb bob"],
    ));
    v.push(p(
        "yaml password",
        "db:\n  password: Yaml Pw 1\n  port: 5432",
        "Yaml Pw 1",
        &["password:", "port: 5432"],
    ));
    v.push(p(
        "yaml quoted",
        "  db_password: \"YamlPw2\"",
        "YamlPw2",
        &["db_password:"],
    ));
    v.push(p(
        "ini password",
        "[db]\npassword = Ini Pw 1\nuser = app",
        "Ini Pw 1",
        &["password ="],
    ));
    v.push(p(
        ".env",
        "DB_PASSWORD=EnvPw1\nDB_PORT=5432",
        "EnvPw1",
        &["DB_PASSWORD=", "DB_PORT=5432"],
    ));
    v.push(p(
        "json password",
        "{\"user\":\"app\",\"password\":\"JsonPw1\"}",
        "JsonPw1",
        &["\"password\":"],
    ));
    v.push(p(
        "json escaped",
        "{\\\"password\\\":\\\"EscPw1\\\"}",
        "EscPw1",
        &["password"],
    ));
    v.push(p(
        "xml password",
        "<config><password>XmlPw1</password></config>",
        "XmlPw1",
        &["<password>"],
    ));
    v.push(p(
        "ado.net",
        "Server=sql01;User Id=sa;Password=AdoPw1;Encrypt=true",
        "AdoPw1",
        &["Password=", "Encrypt=true"],
    ));
    v.push(p(
        "ado Pwd",
        "Driver={x};Uid=sa;Pwd=AdoPw2;",
        "AdoPw2",
        &["Pwd="],
    ));
    v.push(p(
        "jdbc query password",
        "jdbc:postgresql://db:5432/app?user=app&password=JdbcPw1&ssl=true",
        "JdbcPw1",
        &["password=", "ssl=true"],
    ));
    v.push(p(
        "postgres url",
        "postgres://app:PgUrlPw@db.internal:5432/app",
        "PgUrlPw",
        &["postgres://"],
    ));
    v.push(p(
        "mysql url",
        "mysql://root:MyUrlPw@10.0.0.5/shop",
        "MyUrlPw",
        &["mysql://"],
    ));
    v.push(p(
        "mongodb+srv url",
        "mongodb+srv://u:MgUrlPw@cluster0.example.net/db",
        "MgUrlPw",
        &["mongodb+srv://"],
    ));
    v.push(p(
        "redis url empty user",
        "redis://:RdUrlPw@redis:6379/0",
        "RdUrlPw",
        &["redis://"],
    ));
    v.push(p(
        "amqp url",
        "amqp://guest:AmqpPw@rabbit:5672/",
        "AmqpPw",
        &["amqp://"],
    ));
    v.push(p(
        "https userinfo",
        "git clone https://bob:HttpPw1@git.example.com/r.git",
        "HttpPw1",
        &["git clone"],
    ));
    v.push(p(
        "password with @ in url",
        "postgres://app:p@ss@db:5432/x",
        "p@ss",
        &["postgres://"],
    ));
    v.push(p(
        "mount cifs",
        "mount -t cifs -o username=bob,password=CifsPw,vers=3.0 //srv/s /mnt",
        "CifsPw",
        &["vers=3.0"],
    ));
    v.push(p(
        "helm --set",
        "helm install pg bitnami/postgresql --set auth.rootPassword=HelmPw1",
        "HelmPw1",
        &["--set auth.rootPassword="],
    ));
    v.push(p(
        "kubectl from-literal",
        "kubectl create secret generic db --from-literal=password=K8sPw1",
        "K8sPw1",
        &["--from-literal=password="],
    ));
    v.push(p(
        "docker -e",
        "docker run -e POSTGRES_PASSWORD=DkPw1 -d postgres:16",
        "DkPw1",
        &["POSTGRES_PASSWORD="],
    ));
    v.push(p(
        "terraform -var",
        "terraform apply -var 'db_password=TfPw1'",
        "TfPw1",
        &["terraform apply"],
    ));
    v.push(p(
        "ansible become pass",
        "ansible-playbook site.yml -e ansible_become_pass=AnsPw1",
        "AnsPw1",
        &["ansible_become_pass="],
    ));
    v.push(p(
        ".pgpass",
        "db.internal:5432:app:app:PgpassPw1",
        "PgpassPw1",
        &[],
    ));
    v.push(p(
        ".netrc",
        "machine api.example.com login bob password NetrcPw1",
        "NetrcPw1",
        &["machine"],
    ));
    v.push(p(
        "yaml block scalar",
        "stringData:\n  password: |\n    BlockPw1\n    BlockPw2\nkind: Secret",
        "BlockPw1",
        &["kind: Secret"],
    ));
    v.push(p(
        "--db-password flag",
        "app --db-password FlagPw1 --port 80",
        "FlagPw1",
        &["--db-password", "--port 80"],
    ));
    v.push(p(
        "az login -p",
        "az login -u bob@example.com -p AzPw1",
        "AzPw1",
        &["az login"],
    ));
    v.push(p(
        "libpq conninfo",
        "psql \"host=db user=app password=LibpqPw dbname=app\"",
        "LibpqPw",
        &["psql"],
    ));
    v.push(p(
        "gitlab oauth2 url",
        "git clone https://oauth2:GlUrlTok9@gitlab.example.com/g/r.git",
        "GlUrlTok9",
        &["git clone"],
    ));
    v.push(p(
        "ftp url",
        "ftp://bob:FtpPw1@files.example.com/pub",
        "FtpPw1",
        &["ftp://"],
    ));
    v.push(p(
        "ruby hash",
        "{ :user => 'app', 'password' => 'RbPw1' }",
        "RbPw1",
        &["password"],
    ));
    v.push(p(
        "python kwargs",
        "connect(host='db', password='PyPw1', port=5432)",
        "PyPw1",
        &["port=5432"],
    ));
    v.push(p(
        "prose token:",
        "use the api token: ProseTok99 to log in",
        "ProseTok99",
        &["to log in"],
    ));
    v.push(p(
        "passphrase key",
        "passphrase: correct horse battery staple",
        "correct horse battery staple",
        &["passphrase:"],
    ));
    v.push(p(
        "mysql conf",
        "[client]\nuser=app\npassword=\"MyCnfPw\"",
        "MyCnfPw",
        &["[client]"],
    ));
    v.push(p(
        "pip index url",
        "pip install x --index-url https://u:PipPw1@pypi.corp.example/simple",
        "PipPw1",
        &["pip install"],
    ));

    // ---------------- tokens ----------------
    let t = |name: &'static str, input: String, secret: String, keep: &[&'static str]| {
        post(name, input, ALL, vec![secret], keep)
    };
    let s = tok(&["s", "k-"], 48, 1);
    v.push(t(
        "openai key",
        format!("OPENAI_API_KEY={s}"),
        s.clone(),
        &["OPENAI_API_KEY="],
    ));
    v.push(t(
        "openai key bare",
        format!("my key is {s} ok"),
        s.clone(),
        &["my key is", "ok"],
    ));
    let s = tok(&["s", "k-proj-"], 64, 2);
    v.push(t("openai project key", format!("key {s}"), s, &[]));
    let s = tok(&["s", "k-ant-api03-"], 80, 3);
    v.push(t(
        "anthropic key",
        format!("x-api-key: {s}"),
        s,
        &["x-api-key:"],
    ));
    let s = format!("{}{}", "s".to_owned() + "k-", gen(HEX, 32, 4));
    v.push(t(
        "deepseek key",
        format!("curl -H \"Authorization: Bearer {s}\" https://api.deepseek.com/chat/completions"),
        s,
        &["Authorization: Bearer", "api.deepseek.com"],
    ));
    let s = tok(&["gh", "p_"], 36, 5);
    v.push(t(
        "github pat classic",
        format!("GITHUB_TOKEN={s}"),
        s,
        &["GITHUB_TOKEN="],
    ));
    let s = tok(&["gh", "o_"], 36, 6);
    v.push(t("github oauth", format!("token {s}"), s, &[]));
    let s = tok(&["github", "_pat_"], 82, 7);
    v.push(t(
        "github fine-grained",
        format!("gh auth login --with-token <<< {s}"),
        s,
        &["gh auth login"],
    ));
    let s = format!(
        "{}-{}-{}-{}",
        "xo".to_owned() + "xb",
        gen("0123456789", 12, 8),
        gen("0123456789", 13, 9),
        b62(24, 10)
    );
    v.push(t(
        "slack bot token",
        format!("SLACK_TOKEN={s}"),
        s,
        &["SLACK_TOKEN="],
    ));
    let s = format!(
        "https://hooks.slack.com/{}/T{}/B{}/{}",
        "services",
        gen(UPPER_NUM, 8, 11),
        gen(UPPER_NUM, 8, 12),
        b62(24, 13)
    );
    v.push(t(
        "slack webhook",
        format!("curl -X POST {s} -d '{{}}'"),
        s,
        &["curl -X POST"],
    ));
    let s = tok(&["gl", "pat-"], 20, 14);
    v.push(t(
        "gitlab pat",
        format!("PRIVATE-TOKEN: {s}"),
        s,
        &["PRIVATE-TOKEN:"],
    ));
    let s = tok(&["AI", "za"], 35, 15);
    v.push(t("google api key", format!("?key={s}&q=1"), s, &["q=1"]));
    let s = format!("{}{}", "AK".to_owned() + "IA", gen(UPPER_NUM, 16, 16));
    v.push(t(
        "aws access key id",
        format!("aws_access_key_id = {s}"),
        s,
        &["aws_access_key_id"],
    ));
    let s = gen(B64, 40, 17);
    v.push(t(
        "aws secret env",
        format!("AWS_SECRET_ACCESS_KEY={s}"),
        s.clone(),
        &["AWS_SECRET_ACCESS_KEY="],
    ));
    v.push(t(
        "aws configure set",
        format!("aws configure set aws_secret_access_key {s}"),
        s.clone(),
        &["aws configure set"],
    ));
    v.push(t(
        "aws credentials ini",
        format!("[default]\naws_secret_access_key = {s}\nregion = eu-west-1"),
        s,
        &["region = eu-west-1"],
    ));
    let s = jwt(18);
    v.push(t("jwt bare", format!("token={s}"), s.clone(), &["token="]));
    v.push(t(
        "kubectl --token=",
        format!("kubectl --token={s} get pods"),
        s.clone(),
        &["get pods"],
    ));
    v.push(t(
        "authorization bearer header",
        format!("Authorization: Bearer {s}"),
        s.clone(),
        &["Authorization: Bearer"],
    ));
    let s = b62(32, 19);
    v.push(t(
        "kubectl --token space",
        format!("kubectl --token {s} get ns"),
        s.clone(),
        &["get ns"],
    ));
    v.push(t(
        "curl basic header",
        "curl -H 'Authorization: Basic dXNlcjpwYXNzd29yZA==' https://api.example.com".into(),
        "dXNlcjpwYXNzd29yZA==".into(),
        &["Authorization: Basic", "https://api.example.com"],
    ));
    v.push(t(
        "json authorization",
        "{\"Authorization\": \"Token abcdef123456\"}".into(),
        "abcdef123456".into(),
        &["Authorization"],
    ));
    v.push(t(
        "proxy-authorization",
        "Proxy-Authorization: Basic Zm9vOmJhcg==".into(),
        "Zm9vOmJhcg==".into(),
        &["Proxy-Authorization"],
    ));
    v.push(t(
        "cookie header",
        "Cookie: session=abc123def; csrftoken=zzz999".into(),
        "abc123def".into(),
        &["Cookie:"],
    ));
    v.push(t(
        "set-cookie",
        "Set-Cookie: sid=SidValue42; Path=/; HttpOnly".into(),
        "SidValue42".into(),
        &["Set-Cookie:"],
    ));
    v.push(t(
        "curl -b cookie",
        "curl -b 'session=CookieVal7' https://x".into(),
        "CookieVal7".into(),
        &["curl -b"],
    ));
    v.push(t(
        "curl --cookie",
        "curl --cookie \"sid=CookieVal8\" https://x".into(),
        "CookieVal8".into(),
        &["--cookie"],
    ));
    let s = b62(32, 20);
    v.push(t(
        "x-api-key header",
        format!("curl -H \"X-Api-Key: {s}\" https://x"),
        s.clone(),
        &["X-Api-Key:"],
    ));
    v.push(t(
        "query access_token",
        format!("https://api.example.com/v1/me?access_token={s}&fields=id"),
        s.clone(),
        &["fields=id"],
    ));
    let sig = gen(HEX, 64, 21);
    let cred = format!("{}{}", "AK".to_owned() + "IA", gen(UPPER_NUM, 16, 22));
    v.push(post(
        "aws presigned url",
        format!(
            "https://b.s3.amazonaws.com/k?X-Amz-Credential={cred}%2F20260101&X-Amz-Signature={sig}"
        ),
        ALL,
        vec![sig, cred],
        &["X-Amz-Signature="],
    ));
    let s = tok(&["hv", "s."], 90, 23);
    v.push(t(
        "vault login",
        format!("vault login {s}"),
        s.clone(),
        &["vault login"],
    ));
    v.push(t(
        "VAULT_TOKEN",
        format!("export VAULT_TOKEN={s}"),
        s,
        &["export VAULT_TOKEN="],
    ));
    let s = tok(&["np", "m_"], 36, 24);
    v.push(t(
        "npm authToken",
        format!("npm config set //registry.npmjs.org/:_authToken={s}"),
        s,
        &["npm config set"],
    ));
    let s = tok(&["h", "f_"], 34, 25);
    v.push(t("huggingface", format!("HF_TOKEN={s}"), s, &["HF_TOKEN="]));
    let s = tok(&["sk", "_live_"], 24, 26);
    v.push(t("stripe", format!("stripe {s}"), s, &[]));
    let s = format!("{}:AA{}", gen("0123456789", 10, 27), gen(B62, 33, 28));
    v.push(t(
        "telegram bot",
        format!("curl https://api.telegram.org/bot{s}/getMe"),
        s,
        &["curl"],
    ));
    v.push(t(
        "docker config auth",
        "{\"auths\":{\"r.local\":{\"auth\":\"Ym9iOnNlY3JldA==\"}}}".into(),
        "Ym9iOnNlY3JldA==".into(),
        &["\"auth\":"],
    ));
    let s = format!("SG.{}.{}", b62(22, 29), b62(43, 30));
    v.push(t("sendgrid", format!("SENDGRID={s}"), s, &[]));
    let s = gen(B64, 86, 31) + "==";
    v.push(t("azure account key", format!("DefaultEndpointsProtocol=https;AccountName=acc;AccountKey={s};EndpointSuffix=core.windows.net"), s, &["AccountName=acc"]));
    let s = b62(40, 32);
    v.push(t(
        "client_secret yaml",
        format!("client_secret: {s}"),
        s.clone(),
        &["client_secret:"],
    ));
    v.push(t(
        "api_key toml",
        format!("api_key = \"{s}\""),
        s.clone(),
        &["api_key ="],
    ));
    v.push(t(
        "discord webhook",
        format!(
            "https://discord.com/api/webhooks/{}/{}",
            gen("0123456789", 18, 33),
            s
        ),
        s,
        &[],
    ));
    let s = tok(&["gh", "p_"], 36, 34);
    v.push(t(
        ".git-credentials",
        format!("https://bob:{s}@github.com"),
        s,
        &["github.com"],
    ));
    let s = b62(32, 35);
    v.push(t(
        "--api-key flag",
        format!("tool --api-key {s} run"),
        s.clone(),
        &["--api-key", "run"],
    ));
    v.push(t(
        "session id",
        format!("SESSIONID={s}"),
        s,
        &["SESSIONID="],
    ));
    let s = tok(&["gl", "rt-"], 24, 36);
    v.push(t(
        "gitlab runner token",
        format!("gitlab-runner register --token {s}"),
        s,
        &["gitlab-runner register"],
    ));
    let s = tok(&["dckr", "_pat_"], 27, 37);
    v.push(t("docker hub pat", format!("DOCKER_PAT={s}"), s, &[]));

    // ---------------- generic high entropy (Standard/Strict) ----------------
    let s = b62(40, 38);
    v.push(post(
        "high entropy bare",
        format!("use {s} for the webhook"),
        REMOTE,
        vec![s],
        &["for the webhook"],
    ));
    let s = gen(B64, 44, 39);
    v.push(post(
        "high entropy base64",
        format!("value {s} end"),
        REMOTE,
        vec![s],
        &["end"],
    ));

    // ---------------- Strict: hosts, IPs, users, DBs ----------------
    v.push(pos(
        "ssh user@ip",
        "ssh admin@10.20.30.40",
        STRICT,
        &["admin", "10.20.30.40"],
        &["ssh <USER_1>@<IP_1>"],
    ));
    v.push(pos(
        "ping ip",
        "ping -c 3 192.168.1.10",
        STRICT,
        &["192.168.1.10"],
        &["ping -c 3 <IP_1>"],
    ));
    v.push(pos(
        "ipv6",
        "curl -g http://[fe80::1ff:fe23:4567:890a]:8080/",
        STRICT,
        &["fe80::1ff:fe23:4567:890a"],
        &["<IP_1>"],
    ));
    v.push(pos(
        "ipv6 plain",
        "ip -6 route add 2001:db8:abcd:12::/64 via 2001:db8::1",
        STRICT,
        &["2001:db8:abcd:12::", "2001:db8::1"],
        &["ip -6 route add"],
    ));
    v.push(pos(
        "fqdn",
        "ssh deploy@web-01.corp.example.internal uptime",
        STRICT,
        &["deploy", "web-01.corp.example.internal"],
        &["uptime"],
    ));
    v.push(pos(
        "psql flags",
        "psql -h db.internal -U app -d billing -c 'select 1'",
        STRICT,
        &["db.internal", " app", "billing"],
        &["psql -h <HOST_1>", "select 1"],
    ));
    v.push(pos(
        "mysql flags",
        "mysql -h 10.0.0.7 -u reporter -D sales",
        STRICT,
        &["10.0.0.7", "reporter", "sales"],
        &["mysql -h"],
    ));
    v.push(pos(
        "USE db",
        "mysql> USE sales_eu;",
        STRICT,
        &["sales_eu"],
        &["USE"],
    ));
    v.push(pos(
        "\\c db",
        "\\c billing_prod",
        STRICT,
        &["billing_prod"],
        &["\\c"],
    ));
    v.push(pos(
        "create database",
        "CREATE DATABASE IF NOT EXISTS billing_v2;",
        STRICT,
        &["billing_v2"],
        &["CREATE DATABASE IF NOT EXISTS"],
    ));
    v.push(pos(
        "home dir",
        "ls /home/alice/projects",
        STRICT,
        &["alice"],
        &["/home/<USER_1>/projects"],
    ));
    v.push(pos(
        "windows home",
        "cd C:\\Users\\alice\\Documents",
        STRICT,
        &["alice"],
        &["Documents"],
    ));
    v.push(pos(
        "ssh_config",
        "Host prod\n  HostName 10.1.1.1\n  User alice\n  Port 2222",
        STRICT,
        &["10.1.1.1", "alice", "Host prod"],
        &["HostName", "Port 2222"],
    ));
    v.push(pos(
        "shell prompt",
        "alice@prod-db:~$ ls -la",
        STRICT,
        &["alice", "prod-db"],
        &["ls -la"],
    ));
    v.push(pos(
        "scp",
        "scp dump.sql bob@backup.corp.lan:/tmp/",
        STRICT,
        &["bob", "backup.corp.lan"],
        &["scp dump.sql", ":/tmp/"],
    ));
    v.push(pos(
        "rsync host:path",
        "rsync -av ./ backup01:/srv/backup/",
        STRICT,
        &["backup01"],
        &["rsync -av ./"],
    ));
    v.push(pos(
        "ssh -J",
        "ssh -J bastion.corp.lan app01",
        STRICT,
        &["bastion.corp.lan", "app01"],
        &["ssh -J"],
    ));
    v.push(pos(
        "db url strict",
        "postgres://app:UrlPw9@db.internal:5432/billing",
        STRICT,
        &["app:", "UrlPw9", "db.internal", "billing"],
        &["postgres://", ":5432/"],
    ));
    v.push(pos(
        "email",
        "contact alice.smith@corp-mail.ru",
        STRICT,
        &["alice.smith", "corp-mail.ru"],
        &["contact"],
    ));
    v.push(pos(
        "nslookup",
        "nslookup api.corp.internal",
        STRICT,
        &["api.corp.internal"],
        &["nslookup"],
    ));
    v.push(pos(
        "url host",
        "curl https://grafana.corp.internal/api/health",
        STRICT,
        &["grafana.corp.internal"],
        &["/api/health"],
    ));
    v.push(pos(
        "redis -h",
        "redis-cli -h cache.internal -p 6380",
        STRICT,
        &["cache.internal"],
        &["-p 6380"],
    ));
    v.push(pos(
        "kube server",
        "kubectl --server=https://10.0.0.1:6443 get nodes",
        STRICT,
        &["10.0.0.1"],
        &["get nodes"],
    ));
    v.push(pos(
        "libpq env",
        "PGHOST=db.internal PGUSER=app_rw PGDATABASE=billing psql",
        STRICT,
        &["db.internal", "app_rw", "billing"],
        &["PGHOST=", "psql"],
    ));
    v.push(pos(
        "json host",
        "{\"host\": \"db.corp.internal\", \"port\": 5432}",
        STRICT,
        &["db.corp.internal"],
        &["\"port\": 5432"],
    ));
    v.push(pos(
        "ado strict",
        "Data Source=sqlsrv01;Initial Catalog=Billing;User ID=reporter",
        STRICT,
        &["sqlsrv01", "Billing", "reporter"],
        &["Initial Catalog="],
    ));
    v.push(pos(
        "ssh -l",
        "ssh -l carol jump.corp.lan",
        STRICT,
        &["carol", "jump.corp.lan"],
        &["ssh -l"],
    ));
    v.push(pos(
        "mongo url db",
        "mongodb://mongo1.internal:27017/orders",
        STRICT,
        &["mongo1.internal", "orders"],
        &["mongodb://"],
    ));

    v
}

struct Neg {
    name: &'static str,
    input: &'static str,
    profiles: &'static [PrivacyProfile],
}

fn neg(name: &'static str, input: &'static str, profiles: &'static [PrivacyProfile]) -> Neg {
    Neg {
        name,
        input,
        profiles,
    }
}

fn negatives() -> Vec<Neg> {
    vec![
        neg("ls", "ls -la /var/log", ALL),
        neg(
            "kubectl get",
            "kubectl get pods -n kube-system -o wide",
            ALL,
        ),
        neg(
            "ssh port",
            "ssh -p 22 -i ~/.ssh/id_ed25519 web1",
            NOT_STRICT,
        ),
        neg("mysql prompt -p", "mysql -u root -p mydb", NOT_STRICT),
        neg(
            "git author",
            "git log --author alice --since=2.weeks",
            NOT_STRICT,
        ),
        neg(
            "commit sha",
            "git show 3f786850e387550fdab836ed7e6dc881de23001b",
            ALL,
        ),
        neg("uuid", "id=123e4567-e89b-12d3-a456-426614174000", ALL),
        neg(
            "docker digest",
            "nginx@sha256:0d17b565c37bcbd895e9d92315a05c1c3c9a29f762b011a10c54a66cd53c9b31",
            NOT_STRICT,
        ),
        neg("pod name", "kubectl logs api-7d9f8b6c5-x2x4z -c app", ALL),
        neg("password_file path", "password_file: /run/secrets/db", ALL),
        neg(
            "password-stdin flag",
            "docker login --password-stdin -u bob",
            NOT_STRICT,
        ),
        neg("env reference", "PGPASSWORD=$DB_PASS psql", NOT_STRICT),
        neg("braced reference", "password: ${DB_PASSWORD}", ALL),
        neg("template var", "psql \"password={{password}}\"", ALL),
        neg(
            "github actions secret",
            "token: ${{ secrets.GITHUB_TOKEN }}",
            ALL,
        ),
        neg("token_type", "token_type: bearer", ALL),
        neg("max_tokens", "max_tokens: 1024", ALL),
        neg("secretName", "secretName: db-credentials", ALL),
        neg("ssh user@ip standard", "ssh admin@10.0.0.5", NOT_STRICT),
        neg("PWD path", "PWD=/home/alice/src", NOT_STRICT),
        neg(
            "loopback",
            "curl http://127.0.0.1:8080/health && nc -l 0.0.0.0 9000",
            ALL,
        ),
        neg("localhost", "curl http://localhost:3000/", ALL),
        neg(
            "github url",
            "git clone https://github.com/org/repo.git",
            ALL,
        ),
        neg("versions", "nginx/1.25.3 build 10.0.19041.1", ALL),
        neg(
            "file names",
            "vim config.yaml main.rs script.sh README.md app.py main.tf id_rsa.pub",
            ALL,
        ),
        neg(
            "ssh public key",
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGl0c19qdXN0X2FfdGVzdF9rZXlfYm9keQ comment",
            NOT_STRICT,
        ),
        neg(
            "fingerprint",
            "ED25519 key fingerprint is SHA256:Xb8KZqk1LwqZ3dYfHfN0mJcVtR2sP9uQwE4aIoLgT7c.",
            ALL,
        ),
        neg(
            "java class",
            "at org.springframework.AbstractSingletonProxyFactoryBean2024v2",
            NOT_STRICT,
        ),
        neg("time", "started at 12:30:45", ALL),
        neg(
            "mac",
            "link/ether 00:1a:2b:3c:4d:5e brd ff:ff:ff:ff:ff:ff",
            ALL,
        ),
        neg("df -h", "df -h && du -sh *", ALL),
        neg("uid:gid", "docker run -u 1000:1000 alpine id", ALL),
        neg("cookie jar", "curl -b cookies.txt https://x", NOT_STRICT),
        neg("pipe", "echo hello | grep h", ALL),
        neg("select", "SELECT * FROM users WHERE id = 1;", ALL),
        neg("author", "author: Jane", ALL),
        neg("k8s label", "selector:\n  key: app\n  value: web", ALL),
        neg("empty password", "password: ", ALL),
        neg(
            "already sanitized",
            "export PGPASSWORD=<PASSWORD_1> && ssh <USER_1>@<IP_2>",
            ALL,
        ),
        neg("python call", "os.path.join(base, name)", ALL),
        neg("chai", "expect(result).to.be.true", ALL),
        neg("tail", "kubectl logs deploy/api --tail=100 -f", ALL),
        neg("ssh-keygen", "ssh-keygen -t ed25519 -C laptop", ALL),
        neg("terraform plan", "terraform plan -out=tfplan", ALL),
        neg(
            "passphrase prompt",
            "Enter passphrase for key '/home/u/.ssh/id_ed25519':",
            NOT_STRICT,
        ),
        neg("sudo prompt", "[sudo] password for alice:", NOT_STRICT),
        neg("prose", "The password is required for this operation.", ALL),
        neg("PATH", "export PATH=$PATH:/usr/local/bin", ALL),
        neg("root user", "ansible all -m ping -u root", ALL),
        neg("mkdir -p", "mkdir -p /tmp/x && sort -k 2 file", ALL),
        neg("python -c", "python3 -c \"print(1)\"", ALL),
        neg(
            "jsonpath",
            "kubectl get secret db -o jsonpath='{.data.password}'",
            ALL,
        ),
        neg("jq", "jq '.token' response.json", ALL),
        neg("redis -n", "redis-cli -n 2 GET user:42", ALL),
        neg(
            "helm set tag",
            "helm upgrade --install app ./chart --set image.tag=v1.2.3",
            ALL,
        ),
        neg("s3 ls", "aws s3 ls s3://my-bucket/", NOT_STRICT),
        neg(
            "openssl x509",
            "openssl x509 -in cert.pem -noout -text",
            ALL,
        ),
        neg("grep", "grep -rn \"TODO\" src/", ALL),
        neg("separator", "-----\n=====", ALL),
        neg("ssh-copy-id", "ssh-copy-id deploy@web1", NOT_STRICT),
        neg("chmod", "chmod 600 ~/.ssh/config", ALL),
        neg("systemctl", "systemctl status nginx --no-pager", ALL),
        neg("url no creds", "https://example.com:8443/path?x=1", ALL),
        neg("markdown", "## Steps\n1. Run `make`\n2. Done", ALL),
        neg("russian prose", "Перезапусти сервис и проверь логи", ALL),
        neg("no-password flag", "tool --no-password --interactive", ALL),
    ]
}

#[test]
fn corpus_positive_cases() {
    let cases = positives();
    let mut checked = 0;
    let mut failures = Vec::new();
    for c in &cases {
        for &profile in c.profiles {
            let mut s = SanitizerSession::new(profile);
            let out = s.sanitize(&c.input);
            let out = out.as_str();
            for secret in &c.secrets {
                if out.contains(secret.as_str()) {
                    failures.push(format!(
                        "[{}] {:?}: secret {:?} leaked\n  in:  {:?}\n  out: {:?}",
                        c.name, profile, secret, c.input, out
                    ));
                }
            }
            for k in &c.keep {
                if !out.contains(k) {
                    failures.push(format!(
                        "[{}] {:?}: expected {:?} in output\n  in:  {:?}\n  out: {:?}",
                        c.name, profile, k, c.input, out
                    ));
                }
            }
            // Fixed point.
            let again = SanitizerSession::new(profile).sanitize(out);
            if again.as_str() != out {
                failures.push(format!(
                    "[{}] {:?}: not a fixed point\n  out:   {:?}\n  again: {:?}",
                    c.name,
                    profile,
                    out,
                    again.as_str()
                ));
            }
            checked += 1;
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(
        cases.len() >= 150,
        "corpus has {} positive cases",
        cases.len()
    );
    eprintln!(
        "sanitizer corpus: {} positive cases, {} checks",
        cases.len(),
        checked
    );
}

#[test]
fn corpus_negative_cases() {
    let cases = negatives();
    let mut failures = Vec::new();
    for c in &cases {
        for &profile in c.profiles {
            let out = SanitizerSession::new(profile).sanitize(c.input);
            if out.as_str() != c.input {
                failures.push(format!(
                    "[{}] {:?}: changed a clean input\n  in:  {:?}\n  out: {:?}",
                    c.name,
                    profile,
                    c.input,
                    out.as_str()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(
        cases.len() >= 60,
        "corpus has {} negative cases",
        cases.len()
    );
    eprintln!("sanitizer corpus: {} negative cases", cases.len());
}

#[test]
fn local_profile_still_redacts_keys_and_passwords() {
    let key = pem("OPENSSH PRIVATE KEY", 900);
    let text = format!(
        "{key}\nmysql -pLocalPw1 x\nPGPASSWORD=LocalPw2\nssh admin@10.0.0.5\nAuthorization: Bearer {}",
        b62(30, 901)
    );
    let out = SanitizerSession::new(PrivacyProfile::Local).sanitize(&text);
    let out = out.as_str();
    assert!(!out.contains("LocalPw1") && !out.contains("LocalPw2"));
    assert!(!out.contains(key.lines().nth(1).unwrap()));
    assert!(!out.contains(&b62(30, 901)));
    // Host metadata is kept for local models.
    assert!(out.contains("admin@10.0.0.5"));
}

#[test]
fn strict_host_context_literals() {
    let mut s = SanitizerSession::new(PrivacyProfile::Strict);
    s.add_host_context(&HostContext {
        name: "billing-db".into(),
        address: "10.9.8.7".into(),
        username: Some("svc_billing".into()),
        ..Default::default()
    });
    let out = s.sanitize("connect to billing-db (10.9.8.7) as svc_billing; billing-dbx stays");
    let out = out.as_str();
    assert!(!out.contains("svc_billing") && !out.contains("10.9.8.7"));
    assert!(
        out.starts_with("connect to <HOST_1> (<IP_1>) as <USER_1>"),
        "{out}"
    );
    assert!(
        out.contains("billing-dbx stays"),
        "word boundaries respected: {out}"
    );
    let r = s.rehydrate("ssh <USER_1>@<HOST_1>");
    assert_eq!(r.text, "ssh svc_billing@billing-db");

    // Standard ignores host literals.
    let mut s = SanitizerSession::new(PrivacyProfile::Standard);
    s.add_host_context(&HostContext {
        name: "billing-db".into(),
        address: "10.9.8.7".into(),
        ..Default::default()
    });
    assert_eq!(s.sanitize("ssh billing-db").as_str(), "ssh billing-db");
}

#[test]
fn placeholders_stable_across_calls_and_categories_numbered_independently() {
    let mut s = SanitizerSession::new(PrivacyProfile::Strict);
    let a = s.sanitize("ssh bob@10.0.0.1 && mysql -pPw1Secret");
    let b = s.sanitize("again 10.0.0.1 and 10.0.0.2 and -pPw1Secret? no: mysql -pPw1Secret");
    assert!(a.as_str().contains("<IP_1>") && a.as_str().contains("<PASSWORD_1>"));
    assert!(b.as_str().contains("<IP_1>") && b.as_str().contains("<IP_2>"));
    assert!(b.as_str().contains("<PASSWORD_1>"));
}

#[test]
fn report_counts_categories_without_values() {
    let mut s = SanitizerSession::new(PrivacyProfile::Strict);
    let (_, r) = s.sanitize_with_report("ssh bob@10.0.0.1; PGPASSWORD=abc123secret psql");
    assert!(r.redacted_secrets());
    assert_eq!(r.count(cc_ai_core::sanitizer::Category::Password), 1);
    assert_eq!(r.count(cc_ai_core::sanitizer::Category::Ip), 1);
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("abc123secret"));
}

#[test]
fn large_input_is_fast() {
    let mut text = String::new();
    for i in 0..2000 {
        text.push_str(&format!(
            "2026-09-26T10:{:02}:00Z INFO request id={} path=/api/v1/items status=200 took=12ms\n",
            i % 60,
            i
        ));
    }
    text.push_str("PGPASSWORD=needle123 psql\n");
    let start = std::time::Instant::now();
    let out = SanitizerSession::new(PrivacyProfile::Strict).sanitize(&text);
    assert!(!out.as_str().contains("needle123"));
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}
