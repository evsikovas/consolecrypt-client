# ConsoleCrypt

[English](README.md) · [Русский](README.ru.md) · [Español](README.es.md) · **简体中文**

ConsoleCrypt 是一款开源的服务器管理客户端。你可以在同一个应用里打开 SSH 终端、通过 SFTP 传输文件、连接 Windows 远程桌面，也可以查看容器或查询数据库。连接配置保存在本地加密保险库中，还可以在受信任的设备之间同步。

[下载客户端](https://consolecrypt.dev/download?lang=zh) · [使用指南](https://consolecrypt.dev/guide?lang=zh) · [GitHub 最新版本](https://github.com/evsikovas/consolecrypt-client/releases/latest)

![SSH 终端旁的监控面板显示 CPU、内存和网络数据。图中使用的是演示主机。](https://raw.githubusercontent.com/evsikovas/consolecrypt-docs/main/assets/guide/terminal-ssh-metrics-zh.png)

## 1.2.5 版有哪些变化

使用 TCPdump 查看两台主机之间的流量及单个数据包。在 SSH 终端中管理 tmux 会话、搜索输出并自定义服务器指标。可在本机或 SSH 主机上为 Kubernetes 构建并发布 amd64 和 arm64 镜像。Android 宽窗口支持侧边导航和工作窗格。

[1.2.5 发行说明](docs/public/RELEASE_1_2_5.md#简体中文) · [历史版本](https://github.com/evsikovas/consolecrypt-client/releases)

## 安装

| 平台 | 安装包与要求 |
| --- | --- |
| Windows x64 | 运行 EXE 安装程序，或解压 ZIP 使用便携版。 |
| macOS · Intel 和 Apple 芯片 | 打开通用 DMG，将 ConsoleCrypt、ConsoleCrypt Terminal 或两者一起拖入“应用程序”文件夹。 |
| Linux x64 | Debian/Ubuntu 使用 DEB，Fedora 使用 RPM。图形会话需要已解锁且支持持久存储的 Secret Service 密钥环。 |
| Android 11 及以上 · ARM64 | 可以直接安装 APK，也可以从 [RuStore](https://www.rustore.ru/catalog/app/io.consolecrypt.consolecrypt) 下载。发行文件中还包含商店专用的 APK 和 AAB。 |
| iOS Simulator | 在安装了 Xcode 的 Mac 上解压通用 ZIP。此包仅供模拟器使用。 |

下载后，计算文件的 SHA-256 校验值，并与清单中同名文件的值进行比对。 [1.2.5 版 SHA-256 校验清单](https://github.com/evsikovas/consolecrypt-client/releases/download/v1.2.5/SHA256SUMS-1.2.5.txt)。

## 首次连接

1. 选择本地配置文件，或连接到同步服务器。创建保险库后，将恢复套件保存在安全的地方。
2. 打开**主机**页面，添加 SSH 或 RDP 主机，填写地址并设置认证方式。
3. 连接时，通过可信渠道核对主机密钥或证书的指纹。通过 SSH 连接 Linux 后，打开监控面板即可查看主机负载。

SSH、RDP 和 SFTP 会话从你的设备直接连接目标主机，也可以经过配置的跳板机。同步服务器负责账户和加密的保险库记录，可以看到电子邮件地址等账户元数据。

## 主要功能

- **终端与文件** 拆分终端窗格，打开本地 Shell 或 SSH 标签页。“仅终端”模式会在本机记住外观设置。只要内置 SSH 客户端与 Linux 主机的会话仍然打开，即使关闭监控面板也会继续采样。面板显示最近五分钟的 CPU 使用率，以及内存、磁盘和网络指标。你可以从同一标签页打开 SFTP。
- **密码** 打开本地或 WebDAV 上的 KeePass 数据库（KDBX 3/4），也可以浏览 HashiCorp Vault 和 OpenBao 的 KV v1/v2 密钥。在命令面板（Ctrl+K 或 ⌘K）中用方向键选中字段，按 Enter 即可复制隐藏的值。保存的 Vault/OpenBao 连接通过个人保险库同步。每个连接都可以单独选择是否同步密码或令牌，生物识别解锁则在各设备上分别设置。
- **Docker 与 Kubernetes** 查看本地或远程 Docker 引擎的日志，管理容器、卷和 Compose 项目。在 Kubernetes 中，可以浏览资源、检查 YAML、查看 Pod 日志、打开容器终端和转发端口。实验性向导支持将无状态容器部署到集群，前提是集群能够拉取所选镜像。
- **数据库** 查看数据库结构，并在 PostgreSQL、MySQL/MariaDB、SQLite、ClickHouse、Redis、MongoDB 和 Cassandra/ScyllaDB 中执行查询。支持直连、SSH 隧道和容器端口转发。
- **Ansible 自动化** 从 Git 导入项目，编辑 playbook 和主机清单，检查更改后在所选主机上运行。配置服务器端 Runner 后，即使关闭客户端，也能按计划执行任务。移动设备使用远程 Runner。
- **远程访问与协作** 通过 RDP 访问 Windows 桌面，在受信任的设备之间同步连接、凭据和命令片段。团队可以将选定的条目分享给已验证的接收者。撤销访问权限后，接收者此前保存的副本仍然保留。
- **保险库与命令行** 创建加密备份，并在各设备上配置解锁方式。Windows 和 macOS 还支持通过兼容的 FIDO2 安全密钥和 PIN 解锁保险库（预览功能）。桌面安装包内含 `consolecrypt-cli`。
- **可选的 AI 功能** 让配置好的服务解释选中的输出或起草命令。受支持的桌面系统也可以运行本地模型。发送前可检查要提供的上下文，执行前请核对命令。

[GitHub 图文指南](https://github.com/evsikovas/consolecrypt-docs/blob/main/guide/zh.md) · [技术参考](https://github.com/evsikovas/consolecrypt-docs/blob/main/docs/public/zh/README.md) · [同步服务器与协议](https://github.com/evsikovas/consolecrypt-server)

## 从源码构建

共用的 Flutter 界面位于 `client/flutter/`，Rust 原生客户端位于 `client/rust/`，保险库的数据模型位于 `crates/models/`。各平台的打包脚本位于 `client/scripts/`。

安装 `rust-toolchain.toml` 指定的 Rust 工具链、Flutter SDK 和目标平台的原生开发工具。构建 macOS 或 iOS 模拟器版本需要装有 Xcode 的 Mac。构建 Windows 版本需要 MSVC 和 Windows SDK。 [构建环境要求](https://github.com/evsikovas/consolecrypt-docs/blob/main/docs/public/zh/BUILDING.md)。

```sh
git clone https://github.com/evsikovas/consolecrypt-client.git
cd consolecrypt-client/client/flutter
flutter pub get
cd ../..
```

| 目标平台 | 在仓库根目录执行的命令 |
| --- | --- |
| macOS | `bash client/scripts/build-macos.sh` |
| Windows | `powershell -File client/scripts/build-windows.ps1 -Installer` |
| Linux | `bash client/scripts/build-linux.sh` |
| Android | `bash client/scripts/build-android.sh` |
| iOS Simulator | `bash client/scripts/build-ios.sh` |

生成的安装包位于 `dist/`。要使用 APK 覆盖更新已安装的 Android 应用，需要使用相同的签名密钥。

## 运行测试

```sh
cargo test --locked --manifest-path crates/Cargo.toml
cargo test --locked --manifest-path client/rust/Cargo.toml
cd client/flutter
flutter pub get
flutter analyze
flutter test
```

部分集成测试还需要原生库、已解锁的系统密钥环或测试服务器。在同一份源码目录中，请分开运行 Flutter 测试和原生构建。

## 许可证与问题反馈

本项目由 Alexander Evsikov 维护。项目自身的代码采用 [AGPL-3.0-only](LICENSE) 许可证，第三方组件保留各自的许可证。

程序错误请通过 [Issues](https://github.com/evsikovas/consolecrypt-client/issues) 反馈。报告安全漏洞时，请遵循[安全政策](SECURITY.md)。提交前请移除凭据、私钥和保险库内容。
