# ConsoleCrypt

[English](README.md) · [Русский](README.ru.md) · **Español** · [简体中文](README.zh.md)

ConsoleCrypt es un cliente de código abierto para administrar servidores. Desde la misma aplicación puedes abrir una terminal SSH, transferir un archivo por SFTP, conectarte a un escritorio de Windows o consultar un contenedor o una base de datos. Las conexiones se guardan en una bóveda local cifrada que puedes sincronizar entre dispositivos de confianza.

[Descargar](https://consolecrypt.dev/download?lang=es) · [Guía de uso](https://consolecrypt.dev/guide?lang=es) · [Última versión en GitHub](https://github.com/evsikovas/consolecrypt-client/releases/latest)

![Sesión SSH con un panel que muestra el uso de CPU y memoria y el tráfico de red. El servidor contiene datos de ejemplo.](https://raw.githubusercontent.com/evsikovas/consolecrypt-docs/main/assets/guide/terminal-ssh-metrics-es.png)

## Novedades de la versión 1.2.5

Consulta el tráfico entre dos hosts con TCPdump y examina cada paquete. Gestiona sesiones tmux, busca texto y elige indicadores del servidor desde el terminal SSH. Compila y publica imágenes amd64 y arm64 para Kubernetes desde un equipo o un host SSH. En ventanas anchas de Android aparecen la navegación lateral y los paneles de trabajo.

[Notas de la versión 1.2.5](docs/public/RELEASE_1_2_5.md#español) · [Historial de versiones](https://github.com/evsikovas/consolecrypt-client/releases)

## Instalación

| Plataforma | Paquete y requisitos |
| --- | --- |
| Windows x64 | Ejecuta el instalador EXE o extrae la versión portátil del ZIP. |
| macOS · Intel y Apple Silicon | Abre el DMG universal. Arrastra ConsoleCrypt, ConsoleCrypt Terminal o ambas aplicaciones a la carpeta Aplicaciones. |
| Linux x64 | Instala el DEB en Debian/Ubuntu o el RPM en Fedora. La sesión gráfica necesita un llavero persistente de Secret Service desbloqueado. |
| Android 11 o posterior · ARM64 | Instala el APK directamente o descarga la aplicación desde [RuStore](https://www.rustore.ru/catalog/app/io.consolecrypt.consolecrypt). La versión también incluye un APK y un AAB específicos para la tienda. |
| iOS Simulator | Extrae el ZIP universal en un Mac con Xcode. Este paquete se ejecuta solo en el simulador. |

Para verificar la descarga, compara la suma SHA-256 del archivo con la que aparece junto a su nombre. [Sumas SHA-256 de la versión 1.2.5](https://github.com/evsikovas/consolecrypt-client/releases/download/v1.2.5/SHA256SUMS-1.2.5.txt).

## Primera conexión

1. Elige un perfil local o conéctate a un servidor de sincronización. Crea la bóveda y guarda el kit de recuperación en un lugar seguro.
2. Abre **Hosts**, añade un host SSH o RDP e indica su dirección y el método de autenticación.
3. Al conectarte, contrasta la huella de la clave del host o del certificado con una fuente de confianza. En una sesión SSH con Linux, abre el panel de monitorización para consultar la carga.

Las sesiones SSH, RDP y SFTP conectan tu dispositivo con el host de destino, directamente o a través del servidor de salto configurado. El servidor de sincronización gestiona las cuentas y las entradas cifradas de la bóveda. Puede ver metadatos de la cuenta, como la dirección de correo electrónico.

## Qué puedes hacer

- **Terminal y archivos.** Divide la terminal en paneles y abre intérpretes locales o pestañas SSH. El modo «Solo terminal» recuerda la apariencia en cada dispositivo. Mientras la sesión SSH integrada con Linux siga abierta, las métricas se siguen recogiendo aunque cierres el panel de monitorización. El gráfico muestra los últimos cinco minutos de uso de CPU junto a las lecturas de memoria, disco y red. Desde la misma pestaña puedes abrir SFTP.
- **Contraseñas.** Abre bases de datos KeePass locales o alojadas en WebDAV (KDBX 3/4), o consulta secretos KV v1/v2 en HashiCorp Vault y OpenBao. En la paleta de comandos (Ctrl+K o ⌘K), elige un campo con las flechas y pulsa Intro para copiar su valor oculto. Las conexiones guardadas de Vault/OpenBao se sincronizan a través de tu bóveda personal. Para cada conexión puedes elegir si se incluye la contraseña o el token. El acceso biométrico se configura en cada dispositivo.
- **Docker y Kubernetes.** Consulta los registros de un motor Docker local o remoto y gestiona contenedores, volúmenes y proyectos de Compose. En Kubernetes puedes explorar recursos, revisar YAML, ver los registros de un Pod, abrir su terminal y reenviar puertos. Un asistente experimental permite desplegar un contenedor sin estado a partir de una imagen que el clúster pueda descargar.
- **Bases de datos.** Explora esquemas y ejecuta consultas en PostgreSQL, MySQL/MariaDB, SQLite, ClickHouse, Redis, MongoDB y Cassandra/ScyllaDB. Conéctate directamente, mediante un túnel SSH o con un reenvío de puerto de un contenedor.
- **Ansible.** Importa proyectos desde Git, edita playbooks e inventarios, revisa los cambios y ejecútalos en los hosts que elijas. Si configuras un runner en el servidor, las tareas programadas se ejecutan aunque cierres el cliente. En el móvil se utiliza un runner remoto.
- **Acceso y colaboración.** Conéctate a escritorios de Windows por RDP. Sincroniza conexiones, credenciales y fragmentos de comandos entre dispositivos de confianza. Los equipos pueden compartir entradas concretas con destinatarios verificados. Revocar el acceso no borra las copias que ya haya guardado el destinatario.
- **Bóveda y línea de comandos.** Crea copias de seguridad cifradas y configura el desbloqueo en cada dispositivo. En Windows y macOS también puedes desbloquear la bóveda con una llave FIDO2 compatible y su PIN (función preliminar). Los paquetes de escritorio incluyen `consolecrypt-cli`.
- **IA opcional.** Pide al proveedor configurado que explique la salida seleccionada o prepare un comando. Los equipos de escritorio compatibles también pueden ejecutar un modelo local. Revisa los datos antes de enviarlos y el comando antes de ejecutarlo.

[Guía ilustrada en GitHub](https://github.com/evsikovas/consolecrypt-docs/blob/main/guide/es.md) · [Referencia técnica](https://github.com/evsikovas/consolecrypt-docs/blob/main/docs/public/es/README.md) · [Servidor de sincronización y protocolo](https://github.com/evsikovas/consolecrypt-server)

## Compilar desde el código fuente

La interfaz compartida de Flutter está en `client/flutter/`, el cliente nativo de Rust en `client/rust/` y los modelos de datos de la bóveda en `crates/models/`. Los scripts de empaquetado para cada plataforma están en `client/scripts/`.

Instala la versión de Rust indicada en `rust-toolchain.toml`, el SDK de Flutter y las herramientas nativas de la plataforma de destino. Para compilar para macOS y el simulador de iOS necesitas un Mac con Xcode. Para Windows necesitas MSVC y el SDK de Windows. [Requisitos de compilación](https://github.com/evsikovas/consolecrypt-docs/blob/main/docs/public/es/BUILDING.md).

```sh
git clone https://github.com/evsikovas/consolecrypt-client.git
cd consolecrypt-client/client/flutter
flutter pub get
cd ../..
```

| Plataforma | Comando desde la raíz del repositorio |
| --- | --- |
| macOS | `bash client/scripts/build-macos.sh` |
| Windows | `powershell -File client/scripts/build-windows.ps1 -Installer` |
| Linux | `bash client/scripts/build-linux.sh` |
| Android | `bash client/scripts/build-android.sh` |
| iOS Simulator | `bash client/scripts/build-ios.sh` |

Los paquetes se guardan en `dist/`. Para actualizar una instalación de Android mediante un APK hace falta la misma clave de firma.

## Ejecutar las pruebas

```sh
cargo test --locked --manifest-path crates/Cargo.toml
cargo test --locked --manifest-path client/rust/Cargo.toml
cd client/flutter
flutter pub get
flutter analyze
flutter test
```

Las pruebas de integración pueden requerir una biblioteca nativa, un llavero desbloqueado o un servidor de pruebas. En una misma copia del repositorio, ejecuta por separado las pruebas de Flutter y la compilación nativa.

## Licencia y notificación de problemas

Alexander Evsikov mantiene el proyecto. El código propio se distribuye bajo la licencia [AGPL-3.0-only](LICENSE). Los componentes de terceros conservan sus respectivas licencias.

Comunica los errores en [Issues](https://github.com/evsikovas/consolecrypt-client/issues). Para informar de una vulnerabilidad, sigue la [política de seguridad](SECURITY.md). Elimina de los informes las credenciales, las claves privadas y el contenido de la bóveda.
