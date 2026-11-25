# ConsoleCrypt

**English** · [Русский](README.ru.md) · [Español](README.es.md) · [简体中文](README.zh.md)

ConsoleCrypt is an open-source client for administering servers. Open an SSH terminal, transfer a file over SFTP, connect to a Windows desktop, or inspect a container or database. You can keep your connections in a local encrypted vault or sync them across trusted devices.

[Download](https://consolecrypt.dev/download?lang=en) · [User guide](https://consolecrypt.dev/guide?lang=en) · [Latest GitHub release](https://github.com/evsikovas/consolecrypt-client/releases/latest)

![An SSH session with CPU, memory and network monitoring alongside the terminal. The host uses demo data.](https://raw.githubusercontent.com/evsikovas/consolecrypt-docs/main/assets/guide/terminal-ssh-metrics-en.png)

## What changed in 1.2.5

Use TCPdump to inspect traffic between two hosts and open an individual packet. Manage tmux sessions, search output and choose server metrics in the SSH terminal. Build and publish amd64 and arm64 images for Kubernetes from a computer or an SSH host. Wide Android windows show the sidebar and workspace panes.

[1.2.5 release notes](docs/public/RELEASE_1_2_5.md#english) · [All releases](https://github.com/evsikovas/consolecrypt-client/releases)

## Installation

| Platform | Package and requirements |
| --- | --- |
| Windows x64 | Run the EXE installer or extract the portable ZIP. |
| macOS · Intel and Apple silicon | Open the universal DMG. Drag ConsoleCrypt, ConsoleCrypt Terminal, or both into Applications. |
| Linux x64 | Use the DEB on Debian/Ubuntu or the RPM on Fedora. The graphical session needs an unlocked, persistent Secret Service keyring. |
| Android 11+ · ARM64 | Install the direct APK or use [RuStore](https://www.rustore.ru/catalog/app/io.consolecrypt.consolecrypt). The release also includes a separate APK and AAB for the store. |
| iOS Simulator | Open the universal Simulator ZIP on a Mac with Xcode. This package runs only in Simulator. |

To verify a download, compare its SHA-256 checksum with the entry for that filename. [SHA-256 checksums for 1.2.5](https://github.com/evsikovas/consolecrypt-client/releases/download/v1.2.5/SHA256SUMS-1.2.5.txt).

## First connection

1. Choose a local profile or connect to a sync server. Create your vault and keep the recovery kit somewhere safe.
2. Open **Hosts**, add an SSH or RDP host, and enter its address and authentication details.
3. Connect and verify the host key or certificate fingerprint against a trusted source. For SSH, open **Monitoring** to see the load on a Linux host.

SSH, RDP and SFTP sessions run from your device to the target, directly or through the jump host you configure. The sync server handles accounts and encrypted vault records. It can see account metadata such as your email address.

## What you can do

- **Terminal and files.** Use split panes, local shells and SSH tabs. Terminal Only remembers its appearance on this device. While a built-in SSH session to Linux is open, monitoring keeps sampling even when its panel is closed. The CPU graph shows the last five minutes alongside memory, disk and network readings. Open SFTP from the same tab.
- **Passwords.** Open a local or WebDAV KeePass database (KDBX 3/4), or browse KV v1/v2 secrets in HashiCorp Vault and OpenBao. In the command palette (Ctrl+K or ⌘K), use the arrow keys to select a masked field and Enter to copy it. Saved Vault/OpenBao connections sync through your personal vault. You decide per connection whether to include the password or token. Biometric unlock is set up on each device.
- **Docker and Kubernetes.** Inspect a local or remote Docker engine, read logs and manage containers, volumes and Compose projects. In Kubernetes, browse resources, review YAML, open Pod logs or a shell, and forward ports. An experimental wizard can deploy a stateless Docker container using an image the cluster can pull.
- **Databases.** Browse schemas and run queries against PostgreSQL, MySQL/MariaDB, SQLite, ClickHouse, Redis, MongoDB and Cassandra/ScyllaDB. Connect directly or through an SSH tunnel or container port forward.
- **Ansible.** Import projects from Git, edit playbooks and inventories, review changes and run them against selected hosts. A configured server runner can handle schedules while the client is closed. Mobile devices use a remote runner.
- **Access and collaboration.** Use RDP to reach a Windows desktop. Sync saved connections, credentials and snippets between trusted devices. Teams can share selected items with verified recipients. Revoking access cannot erase a copy someone already saved.
- **Vault and command line.** Keep encrypted backups and configure device unlock. On Windows and macOS you can also unlock the vault with a supported FIDO2 security key and PIN (preview). Desktop installers include `consolecrypt-cli`.
- **Optional AI.** Explain selected output or draft a command with a configured provider. Supported desktops can also run a local model. Review the context before sending it and the command before executing it.

[Illustrated guide on GitHub](https://github.com/evsikovas/consolecrypt-docs/blob/main/guide/en.md) · [Technical reference](https://github.com/evsikovas/consolecrypt-docs/blob/main/docs/public/en/README.md) · [Sync server and protocol](https://github.com/evsikovas/consolecrypt-server)

## Build from source

The shared Flutter interface lives in `client/flutter/`, the native Rust client in `client/rust/`, and the vault data models in `crates/models/`. Platform packaging scripts are in `client/scripts/`.

Install the Rust toolchain pinned in `rust-toolchain.toml`, the Flutter SDK and the native tools for your target. macOS and iOS Simulator builds require a Mac with Xcode. Windows builds require MSVC and the Windows SDK. [Build requirements](https://github.com/evsikovas/consolecrypt-docs/blob/main/docs/public/en/BUILDING.md).

```sh
git clone https://github.com/evsikovas/consolecrypt-client.git
cd consolecrypt-client/client/flutter
flutter pub get
cd ../..
```

| Target | Command from the repository root |
| --- | --- |
| macOS | `bash client/scripts/build-macos.sh` |
| Windows | `powershell -File client/scripts/build-windows.ps1 -Installer` |
| Linux | `bash client/scripts/build-linux.sh` |
| Android | `bash client/scripts/build-android.sh` |
| iOS Simulator | `bash client/scripts/build-ios.sh` |

Find the packages under `dist/`. Updating an installed Android APK in place requires the same signing key.

## Run the tests

```sh
cargo test --locked --manifest-path crates/Cargo.toml
cargo test --locked --manifest-path client/rust/Cargo.toml
cd client/flutter
flutter pub get
flutter analyze
flutter test
```

Integration tests may also need a native library, an unlocked keyring or a test server. Run Flutter tests and native builds separately in a given checkout.

## License and reporting issues

The project is maintained by Alexander Evsikov. First-party code is licensed under [AGPL-3.0-only](LICENSE). Third-party components retain their own licenses.

Report bugs in [Issues](https://github.com/evsikovas/consolecrypt-client/issues). For a security vulnerability, follow the [security policy](SECURITY.md). Remove credentials, private keys and vault contents from any report.
