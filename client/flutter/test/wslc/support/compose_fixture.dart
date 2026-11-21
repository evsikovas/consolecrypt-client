// A small Compose project as the Rust core would plan it; no WSLc involved.
import 'package:consolecrypt/wslc/src/rust/models.dart';

const demoComposeYaml = '''name: shop
services:
  db:
    image: postgres:16
    environment:
      POSTGRES_PASSWORD: \${DB_PASS}
    volumes:
      - data:/var/lib/postgresql/data
    networks: [back]
    healthcheck:
      test: ["CMD", "pg_isready"]
      interval: 5s
  web:
    build: ./web
    restart: unless-stopped
    ports:
      - "8080:80"
    volumes:
      - ./site:/usr/share/nginx/html:ro
    networks: [default, back]
    depends_on:
      db:
        condition: service_healthy
volumes:
  data:
networks:
  back:
''';

RunSpec _spec(
  String image,
  String name, {
  List<PortSpec> ports = const [],
  List<MountSpec> mounts = const [],
  List<EnvVar> env = const [],
  required String network,
}) => RunSpec(
  image: image,
  name: name,
  ports: ports,
  mounts: mounts,
  env: env,
  command: const [],
  network: network,
  autoRemove: false,
  start: false,
);

ComposePlan demoComposePlan() => ComposePlan(
  project: 'shop',
  workingDir: r'D:\projects\shop',
  networks: const [
    ComposeNetwork(
      key: 'default',
      isExternal: false,
      spec: NetworkSpec(name: 'shop_default', internal: false, labels: []),
    ),
    ComposeNetwork(
      key: 'back',
      isExternal: false,
      spec: NetworkSpec(name: 'shop_back', internal: false, labels: []),
    ),
  ],
  volumes: const [
    ComposeVolume(
      key: 'data',
      isExternal: false,
      spec: VolumeSpec(name: 'shop_data', options: [], labels: []),
    ),
  ],
  services: [
    ComposeService(
      service: 'db',
      containerName: 'shop-db-1',
      image: 'postgres:16',
      pullPolicy: 'missing',
      spec: _spec(
        'postgres:16',
        'shop-db-1',
        network: 'shop_back',
        env: const [EnvVar(key: 'POSTGRES_PASSWORD', value: 's3cret')],
        mounts: const [
          MountSpec(volume: true, source: 'shop_data', target: '/var/lib/postgresql/data', readOnly: false),
        ],
      ),
      extraNetworks: const [],
      dependsOn: const [],
      configHash: 'db-hash',
      commands: const ['wslc create --name shop-db-1 --env-file <файл-окружения> postgres:16', 'wslc start shop-db-1'],
    ),
    ComposeService(
      service: 'web',
      containerName: 'shop-web-1',
      image: 'shop-web',
      build: const BuildSpec(
        context: r'D:\projects\shop\web',
        tags: ['shop-web'],
        buildArgs: [],
        noCache: false,
        pull: false,
      ),
      pullPolicy: 'missing',
      spec: _spec(
        'shop-web',
        'shop-web-1',
        network: 'shop_default',
        ports: const [PortSpec(hostIp: '0.0.0.0', hostPort: 8080, containerPort: 80, udp: false)],
        mounts: const [
          MountSpec(volume: false, source: r'D:\projects\shop\site', target: '/usr/share/nginx/html', readOnly: true),
        ],
      ),
      extraNetworks: const [
        ComposeAttach(network: 'shop_back', aliases: ['web']),
      ],
      dependsOn: const [ComposeDependency(service: 'db', condition: 'healthy')],
      configHash: 'web-hash',
      commands: const [
        r'wslc build --tag shop-web D:\projects\shop\web',
        'wslc create --name shop-web-1 --publish 0.0.0.0:8080:80 shop-web',
        'wslc network connect --network-alias web shop_back shop-web-1',
        'wslc start shop-web-1',
      ],
    ),
  ],
  hostFolders: const [r'D:\projects\shop\site'],
  commands: const ['wslc network create shop_default', 'wslc network create shop_back', 'wslc volume create shop_data'],
  warnings: const [
    'web: restart: unless-stopped — у WSLc нет политик перезапуска, контейнер сам не перезапустится',
    'Порты без IP опубликованы на всех интерфейсах (0.0.0.0), как в Docker; для доступа только с этого компьютера укажите 127.0.0.1:порт:порт',
  ],
);
