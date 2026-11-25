// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Chinese (`zh`).
class AppLocalizationsZh extends AppLocalizations {
  AppLocalizationsZh([String locale = 'zh']) : super(locale);

  @override
  String get systemdOriginal => '原始消息';

  @override
  String get systemdFilter => '服务状态';

  @override
  String get systemdAll => '所有状态';

  @override
  String get systemdRunning => '运行中';

  @override
  String get systemdActive => '已激活';

  @override
  String get systemdInactive => '未激活';

  @override
  String get systemdFailed => '失败';

  @override
  String get systemdChanging => '状态变化中';

  @override
  String get systemdSort => '服务排序';

  @override
  String get systemdSortName => '名称升序';

  @override
  String get systemdSortReverse => '名称降序';

  @override
  String get systemdSortState => '失败优先';

  @override
  String get systemdUnitPath => '单元文件';

  @override
  String get systemdNoUnitPath => '磁盘上没有单元文件';

  @override
  String get systemdReadUnit => '查看单元文件';

  @override
  String get systemdHideUnit => '收起单元文件';

  @override
  String get systemdLogSearch => '搜索日志';

  @override
  String get systemdErrors => '错误';

  @override
  String get systemdWarnings => '警告';

  @override
  String get systemdInformation => '信息';

  @override
  String get systemdDebug => '调试';

  @override
  String get systemdNoMatchingLogs => '没有匹配的日志记录';

  @override
  String get systemdTitle => 'systemd 服务';

  @override
  String get systemdSearch => '搜索服务';

  @override
  String get systemdEmpty => '未找到服务';

  @override
  String get systemdJournal => '日志 · 最近 200 条';

  @override
  String get systemdFollowJournal => '跟踪日志';

  @override
  String systemdFollowJournalHint(String unit) {
    return '自动显示 $unit 的最新日志。';
  }

  @override
  String get systemdNoEntries => '此 SSH 账户没有可见的日志条目。';

  @override
  String systemdConfirm(String unit, String host) {
    return '要在 $host 上重启 $unit 吗？服务可能会暂时不可用。';
  }

  @override
  String get systemdSudo => '使用免密 sudo';

  @override
  String get systemdRestarted => '服务已重启。';

  @override
  String get systemdFailure => '服务器未能完成操作。';

  @override
  String get systemdTimedOut => '服务器响应超时。请刷新以检查服务状态。';

  @override
  String get systemdUnavailable => '此主机上没有正在运行的 systemd 系统管理器。';

  @override
  String get portCheckTitle => '端口扫描器';

  @override
  String get portCheckSubtitle => '从此设备或已保存的 SSH 服务器检测 TCP 或 UDP 端口。';

  @override
  String get portCheckLocal => '此设备';

  @override
  String get portCheckRemote => 'SSH 服务器';

  @override
  String get portCheckSource => '检测源';

  @override
  String get portCheckTarget => '目标（主机名或 IP）';

  @override
  String get portCheckChoose => '选择服务器';

  @override
  String get portCheckPort => '端口或范围';

  @override
  String get portCheckInvalidInterval => '请输入 1–65535 之间的端口，或输入范围，例如 22-10000。';

  @override
  String get portCheckList => '显示端口';

  @override
  String get portCheckRangeHelp => '点击端口以测试连接并查看服务详情。';

  @override
  String portTypicalService(String service) {
    return '此端口的常见用途：$service';
  }

  @override
  String portOwner(String names) {
    return '监听进程：$names';
  }

  @override
  String get portOwnerUnavailable => '当前权限或系统工具无法获取进程名称。';

  @override
  String get portOwnerReading => '正在读取进程名称…';

  @override
  String get portReasonRefused => '目标拒绝了连接。服务可能监听其他地址，也可能是防火墙拒绝了连接。';

  @override
  String get portReasonUnreachable => '请检查从所选来源到目标的路由和此端口的防火墙规则。';

  @override
  String get portWebsite => 'Web 服务';

  @override
  String get portWebsiteReading => '正在读取首页…';

  @override
  String get portWebsiteTls => '无法建立可信的 TLS 连接。';

  @override
  String get portWebsiteUnavailable => '此服务未通过该协议返回可读的 Web 响应。';

  @override
  String get portRemoteLoopback => '此地址仅在 SSH 服务器本地可用。请通过端口转发在本设备上打开。';

  @override
  String get portBrowserFailed => '无法打开浏览器。你可以复制上面的地址。';

  @override
  String get portCheckNeedsTarget => '请选择已保存的 SSH 目标主机，或输入本设备地址以读取其套接字列表。';

  @override
  String get portCheckChooseTarget => '选择目标主机';

  @override
  String get portCheckResolving => '正在确定目标主机…';

  @override
  String portCheckReading(String protocol) {
    return '正在读取目标主机的 $protocol 套接字…';
  }

  @override
  String portCheckChecking(int port) {
    return '正在检查端口 $port…';
  }

  @override
  String portCheckCompleted(int done, int total) {
    return '已检查 $done/$total 个端口';
  }

  @override
  String get portCheckEmpty => '目标主机在该范围内未报告此协议的套接字。';

  @override
  String portCheckFound(int count) {
    return '范围内的端口数：$count';
  }

  @override
  String get portCheckAction => '检测端口';

  @override
  String get portCheckRemoteHelp => 'SSH 服务器需要 Python 3 和 POSIX shell。目标地址在该服务器上解析。';

  @override
  String get portCheckUdpHelp => '发送一个包含单个零字节的 UDP 数据报，最多等待 5 秒。';

  @override
  String get portCheckRunning => '正在检测…';

  @override
  String get portCheckConnected => 'TCP 连接已建立';

  @override
  String get portCheckReply => '已收到 UDP 响应';

  @override
  String get portCheckRefused => '连接被拒绝';

  @override
  String get portCheckNoReply => '未收到 UDP 响应。开放的端口也可能不响应。';

  @override
  String get portCheckTimeout => '连接超时';

  @override
  String get portCheckDns => '无法解析目标地址';

  @override
  String get portCheckUnreachable => '网络或主机不可达';

  @override
  String get portCheckSocketError => '套接字错误。请检查地址和网络设置。';

  @override
  String get portCheckPython => 'SSH 服务器上未找到 Python 3';

  @override
  String get portCheckRemoteError => 'SSH 服务器未能完成检测';

  @override
  String get portCheckBadHost => '请输入一个主机名或 IPv4/IPv6 地址';

  @override
  String get portCheckProtocol => '协议';

  @override
  String registryBuildOnSsh(String host) {
    return '通过 SSH 在 $host 上构建';
  }

  @override
  String get registryRemoteDirectory => '服务器上包含 Dockerfile 的目录';

  @override
  String get registryRemoteAuthHelp => '构建期间，Buildx 会在此服务器上使用选定的镜像仓库帐户。';

  @override
  String get registryRemoteTools => 'SSH 服务器需要 Python 3、Docker Buildx 以及本地 Docker 套接字访问权限。';

  @override
  String get registryLabel => '镜像仓库';

  @override
  String get registryChoose => '选择已保存的仓库';

  @override
  String get registryAdd => '添加镜像仓库';

  @override
  String get registryOther => '其他仓库';

  @override
  String get registryAddress => '仓库主机';

  @override
  String get registryRepo => '镜像路径';

  @override
  String get registryTag => '标签';

  @override
  String get registryToken => '密码或访问令牌';

  @override
  String get registryPublish => '推送镜像';

  @override
  String get registryPublished => '镜像已发布。镜像字段已填入所选标签。';

  @override
  String get registryInvalid => '请输入仓库主机，可附加端口。';

  @override
  String get registrySaveFailed => '无法保存仓库凭据。';

  @override
  String get registryPublishFailed => '推送失败。请检查镜像路径、推送权限以及 Docker 主机到仓库的连接。';

  @override
  String get terminalBarItems => '状态栏指标';

  @override
  String get terminalBarColor => '指标颜色';

  @override
  String get terminalBarReset => '恢复默认指标和颜色';

  @override
  String get captureSwap => '交换服务器';

  @override
  String get captureSudoPassword => 'sudo 密码';

  @override
  String get captureSyn => '请求建立 TCP 连接';

  @override
  String get captureSynAck => '确认 TCP 连接请求';

  @override
  String get captureAck => '确认收到数据';

  @override
  String get captureData => '传输 TCP 数据';

  @override
  String get captureFin => '结束发送数据';

  @override
  String get captureRst => '重置 TCP 连接';

  @override
  String get captureArpQuery => '查询设备的 MAC 地址';

  @override
  String get captureArpReply => '返回设备的 MAC 地址';

  @override
  String get capturePacketBytes => '捕获的字节 · HEX / ASCII';

  @override
  String get dockerTaskCount => '任务数';

  @override
  String get dockerNetworkId => '网络 ID';

  @override
  String get captureInvalidPassword => 'sudo 密码含有控制字符。请使用单行输入密码。';

  @override
  String get dockerDiskUsage => '磁盘用量';

  @override
  String get dockerEventJournal => '事件';

  @override
  String get dockerBuildCache => '构建缓存';

  @override
  String dockerObjectsCount(int count) {
    return '对象数：$count';
  }

  @override
  String dockerReclaimable(String size) {
    return '可回收：$size';
  }

  @override
  String get dockerRecentLifecycle => '容器事件 · 最近 24 小时';

  @override
  String get dockerJournalEmpty => '最近 24 小时内没有容器事件';

  @override
  String dockerLifecycleAction(String action) {
    String _temp0 = intl.Intl.selectLogic(action, {
      'create': '已创建',
      'start': '已启动',
      'restart': '已重启',
      'stop': '已停止',
      'die': '已退出',
      'kill': '终止信号',
      'oom': '内存不足',
      'destroy': '已删除',
      'pause': '已暂停',
      'unpause': '已恢复',
      'rename': '已重命名',
      'healthy': '健康',
      'unhealthy': '不健康',
      'starting': '正在进行健康检查',
      'other': '事件',
    });
    return '$_temp0';
  }

  @override
  String get keepassRecentKdbx => '最近打开的 KDBX 文件';

  @override
  String get terminalMoreMetrics => '更多服务器指标';

  @override
  String get terminalMetricsBar => '在终端底部显示服务器指标';

  @override
  String get terminalChannelLatency => 'SSH 通道延迟';

  @override
  String terminalLatencyValue(int value) {
    return '$value 毫秒';
  }

  @override
  String get terminalNormalizedLoad => '每个 CPU 的一分钟平均负载';

  @override
  String get terminalEncodingError => '当前编码无法表示输入内容。请选择其他编码后再发送。';

  @override
  String get terminalEncodingSimplifiedChinese => '简体中文';

  @override
  String get terminalEncodingTraditionalChinese => '繁体中文';

  @override
  String get terminalEncodingJapanese => '日语';

  @override
  String get terminalEncodingKorean => '韩语';

  @override
  String get terminalEncodingWesternEuropean => '西欧';

  @override
  String get terminalEncodingCentralEuropean => '中欧';

  @override
  String get terminalFindText => '在终端中查找';

  @override
  String get dockerNetworkContainers => '已连接的容器';

  @override
  String get dockerNetworkEmpty => '未找到网络';

  @override
  String get terminalEncodingCyrillic => '西里尔文';

  @override
  String dockerImageWeeksAgo(int count) {
    return '$count 周前';
  }

  @override
  String get sessionDiskIO => '磁盘读 / 写';

  @override
  String get sessionGroupTabs => '按主机文件夹分组会话';

  @override
  String get sessionDevicesStale => '无法刷新信任状态，当前显示上次的设备列表。';

  @override
  String get sessionRemote => '远程';

  @override
  String get sessionLocal => '本地';

  @override
  String get sessionResponding => '正在回复';

  @override
  String get sessionNoProvider => '选择提供方';

  @override
  String get sessionHidden => '已隐藏';

  @override
  String get sessionHistoryUnavailable => '无法读取命令历史记录';

  @override
  String get serverShellHistory => '服务器命令历史';

  @override
  String get serverHistoryEmpty => 'Shell 历史文件中暂无已保存的命令';

  @override
  String get dockerMainPid => '主进程 PID';

  @override
  String get dockerProcessElapsed => '运行时间';

  @override
  String get dockerProcessUser => '用户';

  @override
  String get sessionHistoryEmpty => '此会话的命令将显示在这里';

  @override
  String get sessionUsage => '资源使用量';

  @override
  String get sessionUsageUnavailable => '指标不可用';

  @override
  String get sessionSwap => '交换空间';

  @override
  String get sessionLogins => '登录会话';

  @override
  String get sessionSnippetSearch => '搜索代码片段';

  @override
  String get sessionAsk => '询问此主机';

  @override
  String get sessionAskGeneral => '提个问题';

  @override
  String get sessionSnip => '存为代码片段';

  @override
  String get uiOtherDevices => '其他设备';

  @override
  String get uiTrustedDevices => '受信任设备';

  @override
  String get uiPlatform => '平台';

  @override
  String get uiLastActive => '最近活动';

  @override
  String get uiTrust => '信任';

  @override
  String get uiDevice => '设备';

  @override
  String get uiPendingRequests => '待确认请求';

  @override
  String get uiVault => '保险库';

  @override
  String get uiProfile => '配置文件';

  @override
  String get uiProtocol => '服务器协议';

  @override
  String get uiOpen => '已解锁';

  @override
  String get uiLocked => '已锁定';

  @override
  String get uiForward => '转发';

  @override
  String get uiTunnel => '隧道';

  @override
  String get uiHost => '主机';

  @override
  String get uiSshTunnels => 'SSH 隧道';

  @override
  String get uiSavedForwards => 'Docker 和 Kubernetes';

  @override
  String get uiLocalForwards => '本机端口转发';

  @override
  String get uiFiveMinutes => '最近 5 分钟';

  @override
  String get uiFieldNavigation => '选择字段';

  @override
  String get dockerKubeRegistryDenied => '镜像仓库拒绝访问。请检查仓库名称，并确认镜像已发布到该仓库。对于私有仓库，请检查集群凭据或此命名空间中的 imagePullSecret。';

  @override
  String get dockerKubeLastProblem => '上次检测到的问题';

  @override
  String get dockerKubeCompletionAdvice =>
      '进程已正常退出。Deployment 会重新启动它以维持一个运行中的副本。请对照 Docker 检查 command 和 args，并使用持续在前台运行的服务进程。一次性任务可使用 Job。';

  @override
  String get dockerKubeOomAdvice => '容器内存不足。请检查内存用量及 Deployment 清单中的内存限制，并查看容器上一次运行的日志。';

  @override
  String get dockerKubeRequestedImage => '清单中的镜像';

  @override
  String get dockerKubeUnverifiedImage =>
      '无法在仓库中确认此镜像的摘要。请输入已发布的摘要或集群可以拉取的其他已发布标签。对于私有仓库，请输入完整镜像引用，并在需要时填写此命名空间中已有的 imagePullSecret。';

  @override
  String get dockerKubePullAdvice => '请在 Pod 事件中查看镜像仓库返回的错误。检查镜像及其标签或摘要是否存在。使用私有仓库时，请检查该命名空间中的 imagePullSecret。';

  @override
  String get dockerKubeCrashAdvice => '请打开 Pod 日志并选择上一次运行的容器。在 Deployment 清单中检查启动命令、参数和内存限制。';

  @override
  String get dockerKubeOpenDeployment => '在 Kubernetes 中打开 Deployment';

  @override
  String get keepassExchangeFormatError => '请检查文件格式。CSV 需要标题和密码列。JSON 必须来自 ConsoleCrypt 导出。';

  @override
  String get keepassExchangeImport => '导入 CSV 或 JSON';

  @override
  String get keepassExchangeCsv => '导出为 CSV';

  @override
  String get keepassExchangeJson => '导出为 JSON';

  @override
  String get keepassExchangePlaintext => '导出的文件包含未加密的密码。';

  @override
  String get keepassExchangeCsvHelp => 'CSV 包含标准字段和组。自定义字段及类似电子表格公式的值请使用 JSON 原样迁移。';

  @override
  String get keepassExchangeJsonHelp => 'JSON 包含可导入 ConsoleCrypt 的标准字段、自定义字段和组。';

  @override
  String get keepassExchangeEmpty => '文件中没有条目。';

  @override
  String keepassExchangeConfirm(int count, String database) {
    return '将 $count 个条目导入 $database？完全相同的条目将被跳过。';
  }

  @override
  String keepassExchangeResult(int added, int skipped) {
    return '已添加 $added 条，已跳过 $skipped 条。';
  }

  @override
  String get monitorTrustHint => '请在终端中打开此主机以验证 SSH 密钥，然后刷新。';

  @override
  String get monitorAuthHint => '请检查此主机已保存的 SSH 凭据，然后刷新。';

  @override
  String get monitorDiskShort => '磁盘';

  @override
  String get monitorMinimal => '切换到极简视图';

  @override
  String get monitorDetailed => '切换到详细视图';

  @override
  String get editorOpenSsh => '打开 SSH 文件夹';

  @override
  String get editorProjectFolder => '项目文件夹';

  @override
  String get editorFontPreview => '编辑器字体预览';

  @override
  String get editorTitle => '编辑器';

  @override
  String get editorOpen => '打开文件夹';

  @override
  String get editorStart => '打开项目文件夹以编辑文件。';

  @override
  String get editorPick => '在项目树中选择一个文件。';

  @override
  String get editorFiles => '文件';

  @override
  String get editorBranches => 'Git 分支';

  @override
  String get editorLocal => '本地分支';

  @override
  String get editorRemote => '远程分支';

  @override
  String get editorNoGit => '此文件夹不在 Git 仓库中。';

  @override
  String get editorNoGitTool => '此电脑上无法使用 Git。';

  @override
  String get editorChanged => '磁盘上的文件已更改。你的编辑仍然保留，请在保存前比较版本。';

  @override
  String get editorUnsupported => '请选择不超过 4 MiB 的 UTF-8 文本文件。';

  @override
  String get editorDiscard => '放弃更改';

  @override
  String get editorUnsaved => '未保存文件';

  @override
  String get editorPlain => '纯文本';

  @override
  String get editorNew => '新建文件';

  @override
  String get editorNoFiles => '空文件夹';

  @override
  String get editorOutside => '请选择项目文件夹中的文件。';

  @override
  String get monitorChoose => '选择主机或组';

  @override
  String get monitorSearch => '搜索主机或组';

  @override
  String get monitorEmpty => '尚未选择监控主机';

  @override
  String get monitorEmptyHint => '点击“+ 主机”选择 Linux 服务器或整个主机组。';

  @override
  String get monitorManual => '手动刷新';

  @override
  String monitorInterval(int seconds) {
    return '每 $seconds 秒';
  }

  @override
  String get monitorPercent => '以百分比显示内存和磁盘使用量';

  @override
  String get monitorAvailable => '可用';

  @override
  String get monitorLoad => '负载';

  @override
  String get hostMetricsTitle => '监控';

  @override
  String get hostMetricsConnecting => '正在通过 SSH 读取首批指标…';

  @override
  String get hostMetricsReadFailed => '无法读取服务器指标。请检查 SSH 连接后重试。';

  @override
  String get hostMetricsLinuxOnly => '此视图显示 Linux 主机的实时指标。';

  @override
  String get hostMetricsNativeOnly => '实时指标需要内置 SSH 客户端。请在主机设置中选择该客户端。';

  @override
  String get hostMetricsLastSample => '更新时间';

  @override
  String get hostMetricsUptime => '运行时间';

  @override
  String hostMetricsUptimeValue(int days, int hours, int minutes) {
    return '$days 天 $hours 小时 $minutes 分钟';
  }

  @override
  String get hostMetricsCpu => 'CPU 使用情况';

  @override
  String get hostMetricsRam => '使用的内存';

  @override
  String get hostMetricsDisk => '使用的系统磁盘';

  @override
  String get hostMetricsNetwork => '网络';

  @override
  String get hostMetricsOs => '操作系统';

  @override
  String get hostMetricsKernel => '内核';

  @override
  String get hostMetricsProcessor => '处理器';

  @override
  String get hostMetricsLoadAverage => '平均负载';

  @override
  String hostMetricsLogicalCpus(int count) {
    return '$count 个逻辑处理器';
  }

  @override
  String get hostMetricsReceived => '接收';

  @override
  String get hostMetricsSent => '发送';

  @override
  String get hostMetricsChanged => '主机配置已更改。请重新连接以刷新指标。';

  @override
  String get hostMetricsConnectSsh => '通过 SSH 连接到服务器以查看其指标。';

  @override
  String get externalVaultTitle => 'Vault / OpenBao';

  @override
  String get externalVaultAdd => '添加连接';

  @override
  String get externalVaultEndpoint => '服务器地址';

  @override
  String get externalVaultAuth => '登录方式';

  @override
  String get externalVaultUserpass => '用户名和密码';

  @override
  String get externalVaultToken => '访问令牌';

  @override
  String get externalVaultApiSettings => 'Vault 设置';

  @override
  String externalVaultKvVersion(int version) {
    return 'KV 版本 $version';
  }

  @override
  String get externalVaultMount => '机密引擎挂载路径';

  @override
  String get externalVaultNamespace => '命名空间（可选）';

  @override
  String get externalVaultAuthMount => 'Userpass 挂载路径';

  @override
  String get externalVaultRemember => '使用 Touch ID 或 Windows Hello 保存登录信息';

  @override
  String get externalVaultUseSaved => '使用设备身份验证登录';

  @override
  String get externalVaultSyncLogin => '将登录凭据保存到我的加密 ConsoleCrypt 保管库';

  @override
  String get externalVaultSyncLoginHelp => '同步后，你可以在任何已解锁个人保管库的设备上使用此密码或令牌。';

  @override
  String get externalVaultSyncedHint => '留空即可使用已同步的登录凭据。';

  @override
  String get externalVaultForgetSynced => '移除已同步的登录凭据';

  @override
  String externalVaultRemoveSyncedConnection(String name) {
    return '要从个人保管库中移除连接“$name”及其登录凭据吗？此更改将同步到所有设备。';
  }

  @override
  String get externalVaultForgetSyncedHelp => '要从个人保管库中移除已保存的密码或令牌吗？此更改将同步到所有设备。';

  @override
  String get externalVaultRemoveConnection => '移除已保存的连接';

  @override
  String get externalVaultNewSecret => '新建机密';

  @override
  String get externalVaultPath => '机密路径';

  @override
  String get externalVaultDeleteVersion => '删除此机密版本？可在 Vault 中恢复。';

  @override
  String get externalVaultDeletePermanent => '永久删除此 KV v1 机密？';

  @override
  String get externalVaultJson => '机密字段（JSON 对象）';

  @override
  String get externalVaultShow => '显示值';

  @override
  String get externalVaultHide => '隐藏值';

  @override
  String get externalVaultInvalidConfig => '请输入以 http:// 或 https:// 开头的服务器地址，并检查 Vault 路径。';

  @override
  String get externalVaultHttpNotice => 'HTTP 会以明文传输凭据和机密值。';

  @override
  String get externalVaultSearch => '搜索当前文件夹或输入机密路径';

  @override
  String get externalVaultEmpty => '此文件夹为空';

  @override
  String get externalVaultNoMatches => '没有匹配的结果';

  @override
  String externalVaultFolderContents(String folder) {
    return '$folder 的内容';
  }

  @override
  String get externalVaultAuthError => 'Vault 未接受这些凭据。';

  @override
  String get externalVaultPermission => '访问被拒绝。请检查 Vault 策略以及令牌是否过期。';

  @override
  String get externalVaultListPermission => '无法列出机密。如有读取权限，请通过完整路径打开。';

  @override
  String get externalVaultNetwork => 'Vault 未响应。请检查连接和服务器地址。';

  @override
  String get externalVaultTls => '请检查服务器证书。如果服务器重定向请求，请配置当前 Vault 地址。';

  @override
  String get externalVaultMissing => '未找到机密。请检查机密路径和挂载配置。';

  @override
  String get externalVaultConflict => '机密已存在、已在服务器上更改，或服务器拒绝了此版本。请重新打开后再保存。';

  @override
  String get externalVaultJsonInvalid => '请输入包含命名字段的有效 JSON 对象。';

  @override
  String get externalVaultBiometricUnavailable => '设备身份验证不可用。您可以使用密码或令牌登录。';

  @override
  String get externalVaultStoreError => '无法使用已保存的登录信息或更新本地设置。请重新输入凭据。';

  @override
  String get externalVaultRequestError => 'Vault 拒绝了请求或返回了意外响应。';

  @override
  String get automationProjectChooseFolderEmpty => '为新项目选择一个空文件夹。要使用已有文件, 请选择打开文件夹。';

  @override
  String get automationGitUnavailable => '在此计算机上安装Git,然后重新打开ConsoleCrypt以导入寄存器.';

  @override
  String get automationProjectRoleHelp => '创建标准Ansible角色布局和调用角色的剧本.';

  @override
  String get automationNewProjectName => '项目名称';

  @override
  String get automationNewProjectType => '项目模板';

  @override
  String get automationProjectPlaybook => '仅Playbook';

  @override
  String get automationProjectRole => 'Ansible 角色和剧本';

  @override
  String get automationProjectRoleName => '角色名称';

  @override
  String get automationProjectProfileFolder => '本地保存ConsoleCrypt 配置文件';

  @override
  String get automationProjectChooseFolder => '选择文件夹...';

  @override
  String get automationProjectUseProfile => '使用配置文件夹';

  @override
  String get automationProjectChooseFolderError => '无法打开文件夹拾取器。';

  @override
  String get automationInventoryRunnerHelp => '预览主机和群名来自INI或YAML. 选择一个运行器以Ansible验证目录并保存.';

  @override
  String get automationInventoryYamlPreviewFailed => '无法读取静态INI 或 YAML 清单。请选择一个清单文件或用 Runner 检查它。';

  @override
  String get automationInventoryYamlPreviewHelp => '来自文件的字面名称. Ansible 在验证过程中解决主机范围和变量.';

  @override
  String get automationInventoryYamlPreviewEmpty => '没有找到主机或组。';

  @override
  String get automationConfigFromFile => '来源 :ansible.cfg 项目根中。窗体加载支持此文件的设置。';

  @override
  String get automationConfigNewFile => '在项目根中保存创建ansible.cfg。';

  @override
  String get keepassMobileFilesHelp => '本地文件被导入到应用程序中并自动保存。使用本地副本导出文件的更改。 WebDAV 直接保存到远程数据库。';

  @override
  String get keepassMoveToGroup => '移动到另一组...';

  @override
  String get tunnelsSaveForward => '保存隧道';

  @override
  String get tunnelsSavedForwardHelp => '保存的Docker和Kubernetes隧道留在这个装置上,在本剖面图中.';

  @override
  String get tunnelsRemoveSavedMessage => '停止此向前并删除此设备中保存的设置 ?';

  @override
  String get tunnelsStartSaved => '开始保存的隧道';

  @override
  String get tunnelsStopSaved => '停止隧道';

  @override
  String get commonCancel => '取消';

  @override
  String get commonSave => '保存';

  @override
  String get commonDelete => '删除';

  @override
  String get commonEdit => '编辑';

  @override
  String get commonRename => '重命名';

  @override
  String get commonClose => '关闭';

  @override
  String get commonCopy => '复制';

  @override
  String get commonBack => '返回';

  @override
  String get commonNext => '下一步';

  @override
  String get commonContinue => '继续';

  @override
  String get commonDone => '完成';

  @override
  String get commonRetry => '重试';

  @override
  String get commonAdd => '添加';

  @override
  String get commonCreate => '创建';

  @override
  String get commonOpen => '打开';

  @override
  String get commonRemove => '移除';

  @override
  String get commonChange => '变动';

  @override
  String get commonShow => '显示';

  @override
  String get commonHide => '隐藏';

  @override
  String get commonSearch => '搜索';

  @override
  String get commonOk => '罢';

  @override
  String get commonReview => '审查';

  @override
  String get commonConnect => '连接';

  @override
  String get commonNever => '永远';

  @override
  String get commonNone => '无';

  @override
  String get commonUnknown => '未知';

  @override
  String get commonName => '名称';

  @override
  String get commonDescription => '说明';

  @override
  String get commonOptional => '可选';

  @override
  String get tagsLabel => '标记';

  @override
  String get tagsHint => '键入标签并按 Enter';

  @override
  String get tagsAdd => '添加标签';

  @override
  String copiedNotice(String what) {
    return '$what复制.';
  }

  @override
  String copiedSecretNotice(String what, int seconds) {
    return '$what复制,剪贴板将在${seconds}s.';
  }

  @override
  String get copyWhatSecret => '机密';

  @override
  String get copyWhatText => '文本';

  @override
  String get copyWhatCommand => '命令';

  @override
  String get copyWhatPublicKey => '公用钥匙';

  @override
  String get copyWhatPassword => '密码';

  @override
  String get copyWhatRecoveryWords => '恢复单词';

  @override
  String get copyWhatFingerprint => '指纹';

  @override
  String get timeJustNow => '刚才';

  @override
  String timeMinutesAgo(int count) {
    return '$count 分钟前';
  }

  @override
  String timeHoursAgo(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '${count}h 从前');
    return '$_temp0';
  }

  @override
  String timeDaysAgo(int count) {
    return '$count 天前';
  }

  @override
  String get timeExpired => '过期';

  @override
  String get timeInLessThanMinute => '在 < 1分钟内';

  @override
  String timeInMinutes(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: ' 在$count分钟)');
    return '$_temp0';
  }

  @override
  String timeInHours(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count h 时');
    return '$_temp0';
  }

  @override
  String timeInDays(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: ' 在$count中');
    return '$_temp0';
  }

  @override
  String get unitBytes => '页:1';

  @override
  String get unitKibibytes => 'KiB 键';

  @override
  String get unitMebibytes => '小贝';

  @override
  String get unitGibibytes => '吉贝';

  @override
  String get unitTebibytes => 'TiB 键';

  @override
  String formatSize(String value, String unit) {
    return '$value $unit';
  }

  @override
  String get languageLabel => '语言';

  @override
  String get languageSystem => '系统';

  @override
  String languageSystemResolved(String language) {
    return '系统（$language）';
  }

  @override
  String get languageSwitcherTooltip => '切换语言';

  @override
  String get languageSettingsHelp => '更改立即生效。“系统”使用操作系统的语言。如果不支持该语言，则使用英语。';

  @override
  String get riskReadOnly => '只读';

  @override
  String get riskModifying => '修改';

  @override
  String get riskDestructive => '破坏性';

  @override
  String get riskUnknown => '未知';

  @override
  String riskSemantics(String level) {
    return '风险:$level';
  }

  @override
  String verificationCodeSemantics(String groups) {
    return '核查编码$groups';
  }

  @override
  String get strengthVeryWeak => '非常虚弱';

  @override
  String get strengthWeak => '弱点';

  @override
  String get strengthFair => '公平';

  @override
  String get strengthStrong => '强';

  @override
  String get strengthVeryStrong => '非常强壮';

  @override
  String strengthLabelWithHint(String label, String hint) {
    return '$label — $hint';
  }

  @override
  String strengthHintEmpty(int min) {
    return '使用至少 4 个随机单词或 $min + 字符';
  }

  @override
  String get strengthHintSequence => '避免键盘或字母顺序';

  @override
  String get strengthHintCommon => '避免常见密码和众所周知的短语';

  @override
  String strengthHintTooShort(int min) {
    return '至少使用$min 字符';
  }

  @override
  String get strengthHintGood => '很好,放在安全的地方,没人能帮你重置';

  @override
  String get strengthHintAddMore => '添加更多随机的单词或字符';

  @override
  String get backupFrequencyDaily => '日报';

  @override
  String get backupFrequencyWeekly => '每周活动';

  @override
  String get aiProviderKindOpenaiCompatible => 'OpenAI-compatible';

  @override
  String get privacyProfileStrict => '严格';

  @override
  String get privacyProfileStandard => '标准';

  @override
  String get privacyProfileLocal => '当地';

  @override
  String get privacyProfileStrictDescription => '隐藏机密信息以及 IP 地址、主机名、用户名和数据库名称';

  @override
  String get privacyProfileStandardDescription => '编辑机密, 保存主机元数据';

  @override
  String get privacyProfileLocalDescription => '对于本地模型: 更多上下文; 秘密仍然被编辑';

  @override
  String get credentialKindPassword => '密码';

  @override
  String get credentialKindSshKey => 'SSH 键';

  @override
  String get credentialKindSshCertificate => 'SSH证书';

  @override
  String get credentialKindOsSshAgent => 'OSSSH剂';

  @override
  String get credentialKindFido2 => 'FIDO2安全钥匙';

  @override
  String get credentialKindExternalAgent => '外部代理';

  @override
  String get hostKeyPolicyAsk => '询问';

  @override
  String get hostKeyPolicyStrict => '严格';

  @override
  String get hostKeyPolicyAcceptNew => '接受新内容';

  @override
  String get hostKeyPolicyAskDescription => '确认未知的主机密钥; 更改的密钥总是失败';

  @override
  String get hostKeyPolicyStrictDescription => '只接受已知主机中的密钥';

  @override
  String get hostKeyPolicyAcceptNewDescription => '自动记录未知密钥; 更改密钥总是失败';

  @override
  String get sshBackendNative => '内置( RUSH)';

  @override
  String get sshBackendOpenSsh => '系统OpenSSH(兼容性)';

  @override
  String get knownHostSourceTofu => '在第一次连接时接受';

  @override
  String get knownHostSourceManual => '手动添加';

  @override
  String get knownHostSourceImported => '从已知主机导入';

  @override
  String get knownHostSourceCertAuthority => '证书授权';

  @override
  String get historyModeLocalOnly => '仅当地';

  @override
  String get historyModeEncryptedSync => '加密同步';

  @override
  String get historyModeDisabled => '已禁用';

  @override
  String get historyModeLocalOnlyDescription => '在此设备上保留历史( 默认)';

  @override
  String get historyModeEncryptedSyncDescription => '历史已同步端到端加密';

  @override
  String get historyModeDisabledDescription => '没有记录命令历史';

  @override
  String get profileKindLocal => '仅当地';

  @override
  String get profileKindSynced => '同步';

  @override
  String profileSyncedSubtitle(String account, String server) {
    return '$account · $server';
  }

  @override
  String get profileUnknownAccount => '账户';

  @override
  String get profileUnknownServer => '服务器';

  @override
  String get enableSyncStepAuthenticating => '签名和注册此设备';

  @override
  String get enableSyncStepCreatingRemoteVault => '在服务器上创建保险库( 相同的保险库 ID)';

  @override
  String get enableSyncStepReconnecting => '服务器上已经存在故障 – 合并';

  @override
  String get enableSyncStepUploading => '上传加密对象';

  @override
  String get enableSyncStepFinishing => '结束';

  @override
  String get enableSyncStepDone => '同步启用';

  @override
  String get enableSyncStepFailed => '失败';

  @override
  String get tunnelKindLocal => '当地';

  @override
  String get tunnelKindRemote => '远程';

  @override
  String get tunnelKindDynamic => '动态( SOCKS5)';

  @override
  String get tunnelKindLocalDescription => '将本地端口转发到服务器可到达的主机';

  @override
  String get tunnelKindRemoteDescription => '在服务器端口显示本地服务';

  @override
  String get tunnelKindDynamicDescription => '通过服务器的本地 SOCKS5 代理';

  @override
  String get snippetSourceUser => '用户';

  @override
  String get snippetSourceAi => '大赦国际';

  @override
  String get snippetSourceImported => '已导入';

  @override
  String get snippetSourceHistory => '历史';

  @override
  String get platformSystemAuthentication => '系统认证';

  @override
  String get platformDeviceNameMac => '我的麦克';

  @override
  String get platformDeviceNamePc => '我的电脑';

  @override
  String get platformDeviceNameOther => '这个设备';

  @override
  String get commandPalette => '命令调色板';

  @override
  String get commandNewTerminalTab => '新建终端标签';

  @override
  String get commandCloseTerminalTab => '关闭终端标签';

  @override
  String get commandNewHost => '新建主机';

  @override
  String get commandLockVault => '锁定默认值';

  @override
  String get commandToggleSidebar => '切换侧边栏';

  @override
  String get commandOpenSettings => '设置';

  @override
  String get commandNewTabPickerTitle => '新建终端标签';

  @override
  String get menuFile => '文件';

  @override
  String get menuView => '视图';

  @override
  String get menuWindow => '窗口';

  @override
  String get navHosts => '主机';

  @override
  String get navGroups => '分组';

  @override
  String get navCredentials => '凭据';

  @override
  String get navTerminal => '终端';

  @override
  String get navSftp => 'SFTP';

  @override
  String get navTunnels => '隧道';

  @override
  String get navSnippets => '代码片段';

  @override
  String get notesShared => '共享注释';

  @override
  String get notesSharedRefresh => '刷新共享笔记';

  @override
  String get notesSharedNew => '新共享说明';

  @override
  String get notesSharedFailed => '无法装入共享的笔记。请检查同步服务器的连接。';

  @override
  String get notesSharedWaiting => '所有者尚未打开此对象的共享笔记。一旦所有者打开笔记面板, 便会出现。';

  @override
  String notesSharedPending(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count 笔记在主人打开笔记面板后即可读取',
      one: '1个音符在所有者打开音符面板后即可读取',
    );
    return '$_temp0';
  }

  @override
  String get notesSharedEmpty => '尚未共享笔记。';

  @override
  String get notesSharedHint => '与你共享此条目的人可以查看备注。只有你可以编辑。';

  @override
  String get notesSharedReadOnly => '只有作者可以更改此注释.';

  @override
  String notesSharedDeleteMessage(String title) {
    return '“$title......”将删除,供任何人查阅。';
  }

  @override
  String get notesAuthorYou => '你们';

  @override
  String get notesAuthorOwner => '拥有者';

  @override
  String get notesAuthorMember => '成员';

  @override
  String get navNotes => '笔记';

  @override
  String get notesThisObject => '对象';

  @override
  String get notesAll => '全部';

  @override
  String get notesTasks => '任务';

  @override
  String get notesSearchHint => '搜索笔记';

  @override
  String get notesNew => '新注释';

  @override
  String get notesEmptyTitle => '还没有笔记';

  @override
  String get notesEmptyMessage => '保留任务、提醒和关于服务器、数据库和容器的记忆。';

  @override
  String notesEmptyObject(String name) {
    return '还没有关于$name';
  }

  @override
  String get notesNothingFound => '没找到';

  @override
  String notesCompleted(int count) {
    return '已完成$count';
  }

  @override
  String get notesTitle => '标题';

  @override
  String get notesBody => '文本 (马克当)';

  @override
  String get notesBodyEmpty => '无文本';

  @override
  String get notesEdit => '编辑';

  @override
  String get notesPreview => '预览';

  @override
  String get notesTags => '标记, 逗号分隔';

  @override
  String get notesTask => '任务';

  @override
  String get notesDue => '到期';

  @override
  String get notesDone => '完成';

  @override
  String get notesReminder => '提醒';

  @override
  String get notesAttachedTo => '附 录';

  @override
  String get notesNoTarget => '未随附';

  @override
  String get notesTargetMissing => '未找到对象';

  @override
  String get notesWhenNone => '无';

  @override
  String get notesWhenInHour => '一小时后';

  @override
  String get notesWhenThisEvening => '今天下午6点';

  @override
  String get notesWhenTomorrow => '明天九点';

  @override
  String get notesWhenNextWeek => '一个星期后';

  @override
  String get notesWhenCustom => '日期和时间...';

  @override
  String get notesWhenCustomHint => 'YYYY-MM-DD HH:MM';

  @override
  String get notesInvalidTime => '输入日期为 YYYY- MM- DD HH: MM';

  @override
  String notesOverdueAt(String when) {
    return '逾期未交——$when';
  }

  @override
  String get notesPin => '键';

  @override
  String get notesUnpin => '撤销';

  @override
  String get notesSnooze => '一小时后提醒';

  @override
  String notesAttachHere(String name) {
    return '附 件 $name';
  }

  @override
  String get notesDetach => '调试';

  @override
  String get notesDelete => '删除注释';

  @override
  String notesDeleteMessage(String title) {
    return '“$title”将删除。';
  }

  @override
  String get notesMarkDone => '标记完成';

  @override
  String get notesMarkOpen => '标记未完成';

  @override
  String get notesBack => '回到笔记';

  @override
  String get notesSaveFailed => '未保存';

  @override
  String get notesSystemReminder => '从ConsoleCrypt发出的教诲,';

  @override
  String notesReminderToast(String title) {
    return '提醒:$title';
  }

  @override
  String get notesOpen => '打开';

  @override
  String get notesHostNotesTitle => '主机卡上的笔记';

  @override
  String get notesHostNotesMove => '移动到注释';

  @override
  String notesFromHostTitle(String name) {
    return '说明:$name';
  }

  @override
  String get navAiChat => 'AI 聊天';

  @override
  String get navDevices => '设备';

  @override
  String get navSync => '同步';

  @override
  String get navBackups => '备份';

  @override
  String get navSettings => '设置';

  @override
  String shellLockVaultTooltip(String shortcut) {
    return '锁库$shortcut';
  }

  @override
  String get shellSearchPlaceholder => '搜索主机,片段或询问AI...';

  @override
  String get shellOfflineTooltip => '同步服务器无法到达。主机、密钥和SSH 继续工作; 更改已排队。';

  @override
  String shellNewTabTooltip(String shortcut) {
    return '新终端标签 $shortcut )';
  }

  @override
  String shellApprovalBannerTitle(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count设备审批请求', one: '请求设备批准');
    return '$_temp0';
  }

  @override
  String shellApprovalBannerMessage(String device, String platform, String ago) {
    return '\" $device \" $platform)要求加入您的保险库$ago. 只有在比较了核查码后才能批准.';
  }

  @override
  String get syncStateLocalOnly => '仅当地';

  @override
  String get syncStateSynced => '同步';

  @override
  String get syncStateSyncedShort => '同步';

  @override
  String syncStateSyncedAgo(String ago) {
    return '同步$ago';
  }

  @override
  String get syncStateSyncing => '同步中...';

  @override
  String get syncStateOffline => '离线';

  @override
  String syncStateOfflinePending(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '离线$count待定');
    return '$_temp0';
  }

  @override
  String get syncStateError => '同步错误';

  @override
  String get syncStatePaused => '同步暂停';

  @override
  String get syncIndicatorLocalTooltip => '此配置文件没有同步。在设置中启用同步。';

  @override
  String syncIndicatorPendingTooltip(String state, int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$state - $count 当地变化等待',
      one: '$state——1个地方变更等待.',
    );
    return '$_temp0';
  }

  @override
  String get errorUnknown => '出现错误，请重试。';

  @override
  String get errorInvalidCredentials => '错误的电子邮件或密码。';

  @override
  String get errorServerUnreachable => '服务器无法访问。请检查 URL 和您的连接。';

  @override
  String get errorIncompatibleServer => '此服务器版本与此应用程序不兼容。更新该应用程序或服务器。';

  @override
  String get errorRegistrationClosed => '此服务器上的注册已关闭。';

  @override
  String get errorEmailTaken => '已存在此电子邮件的账户。';

  @override
  String get errorWrongPassphrase => '口令不正确。';

  @override
  String get errorInvalidRecoveryKey => '此回收密钥不打开保险库。';

  @override
  String get errorDeviceNotTrusted => '此设备尚未信任保险库。';

  @override
  String get errorOsAuthFailed => '系统身份验证失败或已取消。请使用保险库口令解锁。';

  @override
  String get errorVerificationMismatch => '核查代码不匹配——拒绝批准.';

  @override
  String get errorRequestExpired => '请求已过期。';

  @override
  String get errorValidation => '检查输入的值。';

  @override
  String get errorNotFound => '未找到。它可能已被删除。';

  @override
  String get errorConflict => '这在别处被更改了,重新装填,再试一次.';

  @override
  String get errorOffline => '您已下线。当连接恢复时, 更改将会重新测试。';

  @override
  String get errorCancelled => '已经取消。';

  @override
  String get errorHostKeyRejected => '主机密钥被拒绝。';

  @override
  String get errorHostKeyChanged => '主机密钥已更改。连接被屏蔽。';

  @override
  String get errorAuthFailed => 'SSH 认证失败.';

  @override
  String get errorUnsupported => '本文不支持这一行动。';

  @override
  String get errorInternal => '内部错误。请再试一次。';

  @override
  String get errorSessionExpired => '您的会话已过期。再次签名。';

  @override
  String get errorDeviceRevoked => '此设备已被取消。再次签名以注册。';

  @override
  String get errorEmailNotVerified => '先检查你的电子邮件地址';

  @override
  String get errorForbidden => '你没有这个机会。';

  @override
  String get errorRateLimited => '太多尝试了 稍后再试';

  @override
  String errorRateLimitedRetry(int seconds) {
    String _temp0 = intl.Intl.pluralLogic(seconds, locale: localeName, other: '太多尝试了,在${seconds}s.');
    return '$_temp0';
  }

  @override
  String get errorPayloadTooLarge => '数据对服务器来说太大了.';

  @override
  String get errorInvalidProof => '密码检查失败。请求被拒绝。';

  @override
  String get errorServerUnavailable => '服务器暂时无法使用。稍后再试。';

  @override
  String get errorReasonNameRequired => '输入名称。';

  @override
  String get errorReasonProfileNameRequired => '输入配置文件名称。';

  @override
  String get errorReasonPasswordRequired => '输入密码。';

  @override
  String get errorReasonKeyPassphraseRequired => '输入密钥口令。';

  @override
  String get errorReasonAgentPathRequired => '输入代理套接字路径或管道名称。';

  @override
  String get errorReasonInvalidEmail => '输入有效的电子邮件地址。';

  @override
  String errorReasonAccountPasswordTooShort(int min) {
    String _temp0 = intl.Intl.pluralLogic(min, locale: localeName, other: '账户密码必须至少是$min字符.');
    return '$_temp0';
  }

  @override
  String get errorReasonWeakPassphrase => '选择更强的口令。';

  @override
  String get errorReasonInvalidBaseUrl => '输入有效的基址。';

  @override
  String get errorReasonChatModelRequired => '输入聊天模式名称。';

  @override
  String get errorReasonTemplateRequired => '输入命令模板。';

  @override
  String errorReasonMissingVariables(String names) {
    return '填写$names';
  }

  @override
  String get errorReasonInvalidKey => '这不是有效的私钥。';

  @override
  String get errorReasonNotACertificate => '不是一条OpenSSH证书线.';

  @override
  String get errorReasonNotAgentCredential => '这不是代理证书';

  @override
  String get errorReasonKeyHasNoPassphrase => '此密钥没有口令。';

  @override
  String get errorReasonUnsupportedKeyAlgorithm => '生成Ed25519或RSA 3072/4096键.';

  @override
  String get errorReasonGroupCycle => '一个团体不能在内部。';

  @override
  String get errorReasonJumpChainEmpty => '跳跃配置需要至少跳一次.';

  @override
  String get errorReasonHostNotFound => '找不到主机。';

  @override
  String get errorReasonCredentialNotFound => '证书不再存在。';

  @override
  String get errorReasonProfileNotFound => '未找到配置文件。';

  @override
  String get errorReasonDeviceNotFound => '未找到设备。';

  @override
  String get errorReasonTunnelNotFound => '隧道没有找到。';

  @override
  String get errorReasonProviderNotFound => '找不到 AI 提供者。';

  @override
  String get errorReasonRequestNotFound => '请求已不复存在。';

  @override
  String get errorReasonSecretNotStored => '此证书没有秘密。';

  @override
  String get errorReasonSessionClosed => '会议闭幕。';

  @override
  String errorReasonDirectoryNotFound(String path) {
    return '没有这样的目录:$path';
  }

  @override
  String get errorReasonDirectoryNotEmpty => '目录不是空的。';

  @override
  String errorReasonAlreadyExists(String name) {
    return '\" $name \" 已经存在。';
  }

  @override
  String get errorReasonSelectFile => '选择一个文件。';

  @override
  String get errorReasonVaultLocked => '保险库被锁住了';

  @override
  String get errorReasonNoActiveProfile => '无活动简介。';

  @override
  String get errorReasonNoVault => '这个档案还没有保险库';

  @override
  String get errorReasonVaultExists => '保险库已经存在。';

  @override
  String get errorReasonNotABackup => '不是aConsoleCrypt备份(或文件缺失).';

  @override
  String errorReasonBackupExtension(String extension) {
    return '备份文件必须结束为. $extension';
  }

  @override
  String get errorReasonBackupFolderRequired => '先选择备份文件夹。';

  @override
  String get errorReasonKeepAtLeastOne => '保持至少一个备份。';

  @override
  String get errorReasonSyncedProfileRequired => '这需要同步配置文件。';

  @override
  String get errorReasonLocalProfileNoAccount => '本地简介没有账户。';

  @override
  String get errorReasonNoDeviceRegistered => '没有设备注册。';

  @override
  String get errorReasonTrustedDeviceRequired => '只有解锁的可信设备才能做到这一点.';

  @override
  String get errorReasonCurrentPassphraseWrong => '当前口令错误。';

  @override
  String get errorReasonCurrentPasswordWrong => '当前密码错误。';

  @override
  String get errorReasonAccountMismatch => '此配置属于不同的账户。';

  @override
  String get validationFieldName => '名称';

  @override
  String get validationFieldAddress => '地址';

  @override
  String get validationFieldPort => '端口';

  @override
  String get validationFieldBindHost => '绑定地址';

  @override
  String get validationFieldBindPort => '绑定端口';

  @override
  String get validationFieldTargetHost => '目标主机';

  @override
  String get validationFieldTargetPort => '目标端口';

  @override
  String validationRequired(String field) {
    return '$field : 需要';
  }

  @override
  String validationNoWhitespace(String field) {
    return '$field:不得包含空格';
  }

  @override
  String validationPortRange(String field) {
    return '$field:必须是1-65535';
  }

  @override
  String validationInvalid(String field) {
    return '$field:无效值';
  }

  @override
  String get validationJumpChainSelf => '宿主不能跳过自己';

  @override
  String get serverUrlEmpty => '输入您的ConsoleCrypt 服务器的 URL';

  @override
  String get serverUrlNotAUrl => '输入完整 URL, 例如 https://sync.example.org';

  @override
  String get serverUrlInsecure => '请使用 https://。仅 localhost 允许使用未加密的 HTTP。';

  @override
  String get loginTitle => '连接到您的服务器';

  @override
  String get loginTitleReauth => '再次签名';

  @override
  String get loginSubtitle => '使用您自托管的 URLConsoleCrypt 服务器。';

  @override
  String get loginSubtitleReauth => '您在此配置文件的会话结束。本地数据保留在此设备上。';

  @override
  String get loginServerUrlLabel => '服务器 URL';

  @override
  String get loginCheckServer => '检查';

  @override
  String loginServerInfoRegistrationOpen(String serverVersion, String protocolVersion) {
    return 'ConsoleCrypt 服务器$serverVersion • 协议$protocolVersion * 注册开放';
  }

  @override
  String loginServerInfoInviteOnly(String serverVersion, String protocolVersion) {
    return 'ConsoleCrypt服务器$serverVersion 协议$protocolVersion 仅邀请';
  }

  @override
  String get loginModeSignIn => '登录';

  @override
  String get loginModeRegister => '创建账户';

  @override
  String get loginEmailLabel => '电子邮件';

  @override
  String get loginEmailRequired => '输入您的电子邮件地址';

  @override
  String get loginPasswordLabel => '账户密码';

  @override
  String get loginPasswordHelperRegister => '至少10个字符 不同于您的保险库口令';

  @override
  String get loginPasswordRequired => '输入密码';

  @override
  String get loginPasswordTooShort => '至少十个字符';

  @override
  String get loginPasswordRepeatLabel => '重复密码';

  @override
  String get loginPasswordsDoNotMatch => '密码不匹配';

  @override
  String get loginDeviceNameLabel => '此设备的名称';

  @override
  String get loginDeviceNameHelper => '在设备列表中显示,例如\"Work MacBook\".';

  @override
  String get loginDeviceNameRequired => '输入设备名称';

  @override
  String get loginSubmitSignIn => '登录';

  @override
  String get loginSubmitRegister => '创建账户';

  @override
  String get loginForgotPassword => '忘记账户密码了吗?';

  @override
  String get loginForgotNeedsServerAndEmail => '先输入服务器 URL 和您的电子邮件';

  @override
  String get loginResetEmailSent => '如果账户存在, 则重设的电子邮件即将到来。重设只恢复账户访问权限 — 您的保险库仍然需要您的口令或恢复密钥。';

  @override
  String get loginPassphraseNeverLeaves => '您的保险库口令从不离开此设备; 服务器只存储加密数据。';

  @override
  String get profileSwitcherTooltip => '切换配置';

  @override
  String get profileSwitcherAddProfileMenu => '添加配置文件...';

  @override
  String get profileSwitcherManageProfiles => '管理配置文件';

  @override
  String profileSwitcherSwitchTo(String name) {
    return '切换到$name';
  }

  @override
  String get profileSwitcherAddProfile => '添加配置文件';

  @override
  String get backupsTitle => '备份';

  @override
  String get backupsSubtitle => '加密的文件.ccbackup 中只包含密码文本和信封—— 可以在任意机器上用口令或恢复密钥替换。';

  @override
  String get backupsRestoreAction => '还原...';

  @override
  String get backupsExportAction => '导出备份...';

  @override
  String backupsSavedSnack(String fileName, String size) {
    return '已保存备份:$fileName $size ).';
  }

  @override
  String get backupsNoCloudCopyTitle => '此配置文件没有云副本';

  @override
  String get backupsNoCloudCopyMessage => '如果丢失此设备, 备份是获取您数据的唯一方法。';

  @override
  String get backupsSyncedMessage => '此配置文件是同步的, 因此您的服务器持有加密副本。离线备份仍然可以防止服务器丢失或意外删除。';

  @override
  String get backupsRecentTitle => '最近备份';

  @override
  String get backupsRecentEmpty => '尚未从此配置文件创建备份。';

  @override
  String backupsObjectCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count对象', one: '$count对象');
    return '$_temp0';
  }

  @override
  String get backupsAutomatic => '自动';

  @override
  String get backupsAutoTitle => '自动备份';

  @override
  String get backupsAutoSubtitle => '将加密备份写入您选择的文件夹( 例如外部驱动器或同步文件夹) 。';

  @override
  String get backupsFolderLabel => '文件夹';

  @override
  String get backupsFolderNotChosen => '未选择';

  @override
  String get backupsChooseFolder => '选择...';

  @override
  String get backupsFrequencyLabel => '频率';

  @override
  String get backupsKeepLabel => '保留';

  @override
  String backupsKeepLast(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: ' 最后备份$count', one: '上次备份$count');
    return '$_temp0';
  }

  @override
  String get backupsLastRunLabel => '上次运行';

  @override
  String get backupsNextRunLabel => '下次运行';

  @override
  String get backupsLastRunFailed => '上次自动备份失败';

  @override
  String backupsWrittenTo(String folder) {
    return '给$folder的备份';
  }

  @override
  String get backupsBackUpNow => '现在退后';

  @override
  String get restoreTitle => '从备份恢复';

  @override
  String get restoreSubtitle => '将加密文件.ccbackup 恢复到此设备上的新本地配置文件。现有配置文件不被触动。';

  @override
  String get restoreFileLabel => '备份文件';

  @override
  String get restoreBrowse => '浏览...';

  @override
  String get restoreVaultIdLabel => '默认标识';

  @override
  String get restoreCreatedLabel => '已创建';

  @override
  String get restoreObjectsLabel => '对象';

  @override
  String restoreObjectsValue(String count, String size) {
    return '$count ( $size )';
  }

  @override
  String get restoreWrittenByLabel => '编剧:';

  @override
  String restoreWrittenByValue(String appVersion, int formatVersion) {
    return 'ConsoleCrypt${appVersion}_格式v$formatVersion';
  }

  @override
  String get restoreUnlockPassphrase => '保险库口令';

  @override
  String get restoreUnlockRecoveryKey => '回收密钥';

  @override
  String get restorePassphraseLabel => '备份的默认口令';

  @override
  String get restoreRecoveryInputLabel => '24个字或QR文本';

  @override
  String get restoreNewPassphraseLabel => '新建保险库口令';

  @override
  String get restoreProfileNameLabel => '配置文件名称';

  @override
  String get restoreProfileNameHint => '例如,个人(恢复)';

  @override
  String get restoreOpenBackup => '打开备份';

  @override
  String get restoreIntoNewProfile => '恢复为新配置文件';

  @override
  String get welcomeDefaultProfileName => '个人';

  @override
  String get welcomeTitle => '欢迎使用 ConsoleCrypt';

  @override
  String get welcomeTitleAddProfile => '添加配置文件';

  @override
  String get welcomeSubtitle => '端到端加密的 SSH 客户端。主机、密钥、密码和代码片段均在此设备上加密，加密密钥完全由你掌控。';

  @override
  String get welcomeLocalTitle => '在此设备上使用（无需账户）';

  @override
  String get welcomeLocalBody => '所有数据均加密保存在此设备上，无需服务器或账户，也不会产生同步流量。你可以稍后启用同步。';

  @override
  String get welcomeProfileNameLabel => '配置文件名称';

  @override
  String get welcomeLocalAction => '在此设备上使用';

  @override
  String get welcomeServerTitle => '连接服务器';

  @override
  String get welcomeServerBody => '通过你自行托管的 ConsoleCrypt 服务器，在设备之间进行端到端加密同步。服务器无法读取你的数据和密钥。';

  @override
  String get welcomeServerAction => '登录或创建账户';

  @override
  String get welcomeRestoreFromBackup => '从备份文件恢复( .ccbackup )';

  @override
  String get welcomeProfilesHint => '几个配置文件可以共存(例如本地的个人保险库和同步的工作保险库);它们之间从侧边栏切换.';

  @override
  String get credentialsTitle => '凭据';

  @override
  String get credentialsSubtitle => '密码和私密密钥作为单独的加密密钥存储,除非你披露,否则永远不显示.';

  @override
  String get credentialsNew => '新证书';

  @override
  String get credentialsNewPassword => '密码';

  @override
  String get credentialsNewGenerateKey => '生成SSH 密钥...';

  @override
  String get credentialsNewImportKey => '导入 OpenSSH 密钥...';

  @override
  String get credentialsNewCertificate => 'SSH 证书.';

  @override
  String get credentialsNewAgent => 'SSH代理人.';

  @override
  String get credentialsEmptyTitle => '尚未有全权证书';

  @override
  String get credentialsEmptyMessage => '生成一个 Ed25519 密钥,导入一个已有的OpenSSH密钥,或存储密码.';

  @override
  String credentialsListUser(String username) {
    return '用户$username';
  }

  @override
  String get credentialsListPassphraseRemembered => '口令已记住';

  @override
  String get credentialsListPassphraseProtected => '口令保护';

  @override
  String credentialsListUsedBy(int count) {
    return '由$count使用';
  }

  @override
  String credentialsDeleteTitle(String name) {
    return '删除$name?';
  }

  @override
  String get credentialsDeleteMessage => '证书及其秘密从保险库中删除。使用它的主机需要另一个证书。';

  @override
  String get credentialsDetailsType => '类型';

  @override
  String get credentialsDetailsUsername => '用户名';

  @override
  String get credentialsDetailsAlgorithm => '算法';

  @override
  String get credentialsDetailsFingerprint => '指纹';

  @override
  String get credentialsDetailsPassphraseRemembered => '被记住( 存储为单独的加密密件)';

  @override
  String get credentialsDetailsPassphraseAsked => '在连接时被问到——钥匙保留自身的保护.';

  @override
  String get credentialsDetailsAgentSocket => '代理套接字';

  @override
  String get credentialsDetailsCreated => '已创建';

  @override
  String get credentialsDetailsPublicKey => '公用钥匙';

  @override
  String get credentialsCopyPublicKey => '复制公钥';

  @override
  String get credentialsDetailsCertificate => '证书';

  @override
  String get credentialsReveal => '启示';

  @override
  String get credentialsRevealHint => '20 s 后再次隐藏已披露的值; 复制的值会自动从剪贴板中清除。';

  @override
  String get credentialDialogNewPasswordTitle => '新密码';

  @override
  String get credentialDialogUsernameOptional => '用户名( 可选)';

  @override
  String get credentialDialogPasswordLabel => '密码';

  @override
  String get credentialDialogKeyPassphrase => '密钥口令';

  @override
  String get credentialDialogRememberPassphrase => '记住SSH密钥口令';

  @override
  String get credentialDialogAgentTitle => 'SSH剂';

  @override
  String get credentialDialogAgentDefaultName => 'SSH剂';

  @override
  String get credentialDialogAgentPathLabel => '套接字路径或管道名称';

  @override
  String credentialDialogAgentPathHint(String unixPath, String windowsPath) {
    return '$unixPath 或 $windowsPath (中文(简体) ).';
  }

  @override
  String get credentialDialogAgentNotice => '钥匙留在经纪人中; ConsoleCrypt 永远看不到.';

  @override
  String get credentialGenerateTitle => '生成SSH 密钥';

  @override
  String get credentialGenerateAction => '生成';

  @override
  String credentialGenerateRecommended(String algorithm) {
    return '$algorithm(建议)';
  }

  @override
  String get credentialGenerateRsaNotice => 'RSA是为了与旧服务器兼容;生成需要几秒钟.';

  @override
  String get credentialGenerateCommentLabel => '注释( 可选)';

  @override
  String get credentialGeneratePassphraseLabel => '密钥口令( 可选)';

  @override
  String get credentialGeneratePassphraseHelper => '除保险库加密外,还保护密钥本身.';

  @override
  String get credentialGenerateRepeatPassphrase => '重复密钥口令';

  @override
  String get credentialGeneratePassphraseMismatch => '口令不匹配';

  @override
  String get credentialGenerateRememberSubtitle => '存储在保险库中作为单独的加密密件, 所以连接时不会询问您。';

  @override
  String get credentialGenerateDoneTitle => '生成的密钥';

  @override
  String get credentialGenerateDoneMessage => '将此公钥添加到服务器上的 ~/. ssh/ 授权_ keys 中 :';

  @override
  String get credentialImportTitle => '导入OpenSSH 密钥';

  @override
  String get credentialImportTitleWithCertificate => '导入密钥 + 证书';

  @override
  String get credentialImportAction => '导入';

  @override
  String get credentialImportChooseFile => '选择文件...';

  @override
  String get credentialImportPrivateKey => '私钥';

  @override
  String get credentialImportUnknownAlgorithm => '密钥';

  @override
  String get credentialImportEncryptedNotice => '口令保护: 它会加密在你的保险库里。';

  @override
  String get credentialImportInvalidKey => '无效的密钥';

  @override
  String get credentialImportRememberSubtitle => '关闭: 每次连接都向您要。打开: 存储为单独的加密密钥。';

  @override
  String get credentialImportCertificateLabel => 'OpenSSH证书';

  @override
  String approveDeviceTitle(String name) {
    return '核准 \" $name \" ?';
  }

  @override
  String approveDeviceStepShowsCode(String name) {
    return '1. 关于 \" $name \" ,ConsoleCrypt,显示核查码。';
  }

  @override
  String get approveDeviceStepCompare => '2. 与这里计算出的代码进行比较,按组:';

  @override
  String approveDeviceCodesMatchCheckbox(String name) {
    return '所有6个组都与 \" $name \" 显示的代码相符。';
  }

  @override
  String get approveDeviceWarning => '批准会给这个设备你的保险库钥匙 永远不要批准你现在没有设置的设备';

  @override
  String get approveDeviceCodesDontMatch => '密码不匹配';

  @override
  String get approveDeviceApprove => '核准';

  @override
  String approveDeviceApproved(String name) {
    return '$name现在可以打开你的保险库';
  }

  @override
  String get approveDeviceRejectedTitle => '请求被驳回';

  @override
  String get approveDeviceRejectedMessage =>
      '代码不匹配, 所以设备不被批准。有人 — — 可能是被损坏的服务器 — — 可能试图在您的账户中添加自己的设备。\n\n 如果您没有启动此程序, 请更改您的账户密码并审查您的设备。';

  @override
  String get devicesTitle => '设备';

  @override
  String devicesRevokeTitle(String name) {
    return '撤销$name?';
  }

  @override
  String get devicesRevokeMessage =>
      '设备立即被签出, 无法进入您的所有保险库, 无法同步。这不能被撤销。要再次使用它, 它必须被批准为新设备。\n\n撤销保护未来,而不是过去:已经解密的设备数据不能被远程删除.';

  @override
  String get devicesRevokeConfirm => '撤销设备';

  @override
  String devicesRevoked(String name) {
    return '$name 撤销';
  }

  @override
  String get devicesRenameTitle => '重命名设备';

  @override
  String get devicesLocalTitle => '适用于同步配置文件的设备';

  @override
  String get devicesLocalMessage => '此配置文件仅是本地文件: 它存在于此设备上, 而在其它设备上也不存在。启用同步, 在设备批准的情况下, 在多个机器上使用保险库。';

  @override
  String get devicesEnableSync => '启用同步...';

  @override
  String devicesSubtitle(String email) {
    return '设备签名到$email 。只有信任的设备可以解密您的保险库。';
  }

  @override
  String get devicesSubtitleNoEmail => '登录到您的账户的设备。只有信任的设备才能解密您的保险库。';

  @override
  String get devicesRefresh => '刷新';

  @override
  String devicesPendingTitle(String name) {
    return '\"$name\"想进入你的保险库';
  }

  @override
  String devicesPendingMeta(String platform, String ago, String remaining) {
    return '$platform · 要求 $ago 过期 $remaining';
  }

  @override
  String devicesPendingMetaExpired(String platform, String ago) {
    return '$platform · 要求 $ago 过期';
  }

  @override
  String get devicesPendingHint => '只有在您正在设置此设备 并且验证码匹配的情况下 才会批准';

  @override
  String get devicesRequestRejected => '请求被驳回';

  @override
  String get devicesReject => '拒绝';

  @override
  String get devicesReviewApprove => '审查和核准';

  @override
  String get devicesStatusRevoked => '已撤销';

  @override
  String get devicesStatusTrusted => '已经信任';

  @override
  String get devicesStatusAwaitingApproval => '等待核准';

  @override
  String get devicesStatusNotTrusted => '此保险库不可信';

  @override
  String get devicesThisDevice => '这个设备';

  @override
  String devicesLastSeen(String ago) {
    return '最后一次见到$ago';
  }

  @override
  String devicesAdded(String date) {
    return '添加$date';
  }

  @override
  String devicesRevokedOn(String date) {
    return '撤销$date';
  }

  @override
  String get devicesRevokeMenu => '撤销...';

  @override
  String get syncScreenTitle => '同步';

  @override
  String get syncScreenLocalSubtitle => '此配置文件没有服务器也没有账户。没有任何设备离开。';

  @override
  String get syncScreenLocalBody => '启用同步在其他机器上使用此保险库。保险库被上传到您自控服务器的端到端加密; 其ID、口令和回收 Kit 保持不变。';

  @override
  String get syncScreenEnableSync => '启用同步...';

  @override
  String get syncScreenBackups => '备份';

  @override
  String syncScreenServer(String url) {
    return '服务器:$url';
  }

  @override
  String get syncScreenSyncNow => '现在同步';

  @override
  String get syncScreenState => '状态';

  @override
  String get syncScreenStateIdle => '闲置';

  @override
  String get syncScreenStateSyncing => '同步中';

  @override
  String get syncScreenStateOffline => '离线';

  @override
  String get syncScreenStateError => '错误';

  @override
  String get syncScreenStatePaused => '暂停';

  @override
  String get syncScreenLastSync => '上次成功同步';

  @override
  String get syncScreenPendingChanges => '待修改';

  @override
  String get syncScreenServerSequence => '服务器序列';

  @override
  String get syncScreenNextRetry => '下次重试';

  @override
  String get syncScreenOfflineMessage => '服务器无法访问。您的主机, 密钥, SSH, SFTP 和隧道继续工作; 更改是本地存储的, 并在服务器返回时按下。';

  @override
  String get syncScreenProblems => '问题';

  @override
  String get syncScreenIssueRetryable => '同步失败—— 将自动重试';

  @override
  String get syncScreenIssue => '同步失败';

  @override
  String get syncScreenDisconnectTitle => '断开连接';

  @override
  String get syncScreenDisconnectBody => '停止同步此配置文件并将其数据保存在本地。';

  @override
  String get syncScreenDisconnect => '断开连接...';

  @override
  String get syncScreenDeveloper => '开发者( 模拟后端)';

  @override
  String get syncScreenSimulateOutage => '模拟服务器退出';

  @override
  String get syncDisconnectTitle => '断开同步?';

  @override
  String get syncDisconnectMessage => '同步停止, 此配置文件将变成本地文件。所有数据都保留在这个设备上。服务器上的加密副本没有被更改。';

  @override
  String get syncDisconnectRevoke => '也在服务器上取消此设备';

  @override
  String get syncDisconnectConfirm => '断开连接( 保留本地数据)';

  @override
  String get enableSyncDialogTitle => '启用同步';

  @override
  String get enableSyncDialogStepServer => '3 步骤 1 —— 您的服务器';

  @override
  String get enableSyncDialogStepAccount => '第2步,第3步——账户';

  @override
  String get enableSyncDialogStepUpload => '● 上传';

  @override
  String get enableSyncDialogIntro => '您的保险库被上传到端到端加密: 服务器只存储密码文本, 并且从未看到您的口令或密钥。保险库保持其ID, 您的回收工具仍然有效。';

  @override
  String get enableSyncDialogServerUrl => '服务器 URL';

  @override
  String enableSyncDialogServerInfo(String serverVersion, String protocolVersion) {
    return 'ConsoleCrypt 服务器 $serverVersion * 协议 $protocolVersion';
  }

  @override
  String get enableSyncDialogCreateAccount => '创建账户';

  @override
  String get enableSyncDialogSignIn => '登录';

  @override
  String get enableSyncDialogEmail => '电子邮件';

  @override
  String get enableSyncDialogPassword => '账户密码';

  @override
  String get enableSyncDialogPasswordHelper => '至少10个字符 不是你的保险库密码';

  @override
  String get enableSyncDialogDeviceName => '此设备的名称';

  @override
  String get enableSyncDialogCredentialsRequired => '输入您的电子邮件和密码';

  @override
  String enableSyncDialogUploadedObjects(int uploaded, int total) {
    String _temp0 = intl.Intl.pluralLogic(total, locale: localeName, other: '$uploaded和$total加密对象');
    return '$_temp0';
  }

  @override
  String get enableSyncDialogDone => '此配置文件现已同步。通过在其中签名, 并用校验代码批准, 添加其它设备。';

  @override
  String get enableSyncDialogFailed => '启用同步失败。';

  @override
  String get enableSyncDialogFailedTitle => '启用同步失败';

  @override
  String get enableSyncDialogStart => '启用同步';

  @override
  String get enableSyncDialogTryAgain => '再试一次';

  @override
  String get glassSetting => '眼镜';

  @override
  String get glassModeClear => '清除';

  @override
  String get glassModeStandard => '默认';

  @override
  String get glassModeTinted => '叮当';

  @override
  String get glassModeSolid => '固态';

  @override
  String get glassCapsLockOn => '机长锁定开启';

  @override
  String get glassDismiss => '开除';

  @override
  String get glassCloseTab => '关闭标签页';

  @override
  String get glassNewTab => '新建标签页';

  @override
  String get glassTabColumnResize => '标签列宽度';

  @override
  String get glassTabColumnCollapse => '将标签折叠到图标';

  @override
  String get glassTabColumnExpand => '扩展标签';

  @override
  String get glassTabConnected => '已连接';

  @override
  String get glassTabReconnecting => '正在重新连接...';

  @override
  String get glassTabDisconnected => '已断开连接';

  @override
  String get glassVerificationCode => '核查代码';

  @override
  String glassCodeGroup(int number, String digits) {
    return '组$number: $digits';
  }

  @override
  String glassClearsIn(int seconds) {
    return '清除于$seconds s';
  }

  @override
  String get glassMoreActions => '更多';

  @override
  String get hostsTitle => '主机';

  @override
  String hostsSubtitle(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count这个保险库里的主机', one: '$count这个保险库里的主机');
    return '$_temp0';
  }

  @override
  String get hostsNewHost => '新建主机';

  @override
  String get hostsSearchHint => '按名称、地址、标签或组搜索';

  @override
  String get hostsEmptyTitle => '暂无主机';

  @override
  String get hostsNoMatches => '没有匹配的主机';

  @override
  String get hostsEmptyMessage => '添加您的第一个SSH 主机连接。';

  @override
  String get hostsAddHost => '添加主机';

  @override
  String get hostsViaJumpHostsTooltip => '通过跳转主机连接';

  @override
  String get hostsOpenSshTooltip => '使用系统OpenSSH后端';

  @override
  String get hostsAssignGroup => '添加到组...';

  @override
  String get hostsGroupHelp => '为这个主机选择一个组。主机上未设置的连接设置是从组中继承的。';

  @override
  String get hostsGroupSearchHint => '找到一组...';

  @override
  String get hostsGroupNoMatches => '没有匹配组';

  @override
  String get hostsMoreTooltip => '更多';

  @override
  String get hostsOpenSftp => '打开SFTP';

  @override
  String hostsDeleteTitle(String name) {
    return '删除$name?';
  }

  @override
  String get hostsDeleteMessage => '主机从此保险库中移除( 以及同步后从您的其他设备中移除) 。使用该主机的跳转链会更新。';

  @override
  String get hostsDeleted => '主机已删除';

  @override
  String get hostsNewCredentialEntry => '+ 新证书...';

  @override
  String get hostEditorLoadingTitle => '主机';

  @override
  String get hostEditorNewTitle => '新建主机';

  @override
  String hostEditorEditTitle(String name) {
    return '编辑 $name';
  }

  @override
  String get hostEditorEditTitleFallback => '编辑主机';

  @override
  String get hostEditorConnectionSection => '连接';

  @override
  String get hostEditorNameRequired => '输入名称';

  @override
  String get hostEditorAddressLabel => '地址';

  @override
  String get hostEditorAddressHint => '主机名称或IP';

  @override
  String get hostEditorAddressRequired => '输入地址';

  @override
  String get hostEditorAddressNoSpaces => '不允许空格';

  @override
  String get hostEditorPortLabel => '端口';

  @override
  String get hostEditorPortHint => '继承';

  @override
  String get hostEditorUsernameLabel => '用户名';

  @override
  String get hostEditorUsernameHint => '从组中继承';

  @override
  String get hostEditorGroupLabel => '组';

  @override
  String get hostEditorNoGroup => '未分组';

  @override
  String get hostEditorJumpSection => '跳转主机';

  @override
  String get hostEditorJumpSubtitle => '多跳:每跳打开一个直通Tcpip频道到下一个频道,对跳次数没有限制.';

  @override
  String get hostEditorJumpInherit => '继承/直接';

  @override
  String get hostEditorJumpProfile => '跳转配置';

  @override
  String get hostEditorJumpCustom => '自定义链';

  @override
  String get hostEditorJumpInheritHelp => '使用组的跳动配置(如果有),否则直接连接。';

  @override
  String hostEditorJumpProfileItem(int count, String name) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$name $count hops (英语).');
    return '$_temp0';
  }

  @override
  String get hostEditorThisHost => '这个主机';

  @override
  String get hostEditorSecuritySection => '警卫';

  @override
  String get hostEditorHostKeyPolicy => '主机密钥政策';

  @override
  String get hostEditorSshBackend => 'SSH后门';

  @override
  String get hostEditorKeepalive => '保持间隔(s)';

  @override
  String get hostEditorKeepaliveHint => '应用程序默认';

  @override
  String get hostEditorAgentForwarding => '代理转发';

  @override
  String get hostEditorAgentForwardingHelp => '允许此服务器使用您的密钥继续连接。只为您信任的主机启用。';

  @override
  String get hostEditorOrganisationSection => '日历';

  @override
  String get hostEditorNotes => '笔记';

  @override
  String get hostEditorEffectiveTitle => '有效设置';

  @override
  String get hostEditorEffectiveSubtitle => '从这个主机及其组链中解析出来——连接将使用什么.';

  @override
  String get hostEditorEffectiveGroup => '组';

  @override
  String get hostEditorEffectiveUsername => '用户名';

  @override
  String get hostEditorEffectivePort => '端口';

  @override
  String get hostEditorEffectiveCredential => '凭据';

  @override
  String get hostEditorEffectiveRoute => '路线';

  @override
  String get hostEditorRouteDirect => '直接';

  @override
  String get hostEditorSourceHost => '在此主机上设置';

  @override
  String hostEditorSourceFrom(String name) {
    return '调自$name';
  }

  @override
  String hostEditorSourceCredential(String name) {
    return '证书$name';
  }

  @override
  String hostEditorSourceGroup(String name) {
    return '从集团继承$name';
  }

  @override
  String hostEditorSourceJumpProfile(String name) {
    return '跳跃简介$name';
  }

  @override
  String get hostEditorSourceDefault => '默认';

  @override
  String get hostEditorSourceUnset => '未设置';

  @override
  String get hostEditorCredentialPrompt => '密码 - 连接时询问';

  @override
  String get hostEditorProblemJumpHostDeleted => '链条中的跳转主机被删除';

  @override
  String get hostEditorProblemSelfJump => '主机不能跳过自己';

  @override
  String get hostEditorProblemNoCredential => '无证书: 连接时会请求密码';

  @override
  String get hostEditorProblemCredentialMissing => '选中的证书已不复存在';

  @override
  String get hostEditorProblemNoUsername => '没有用户名: 在主机或组上设置一个';

  @override
  String get hostAuthModePassword => '密码';

  @override
  String get hostAuthModeSshKey => 'SSH 键';

  @override
  String get hostAuthModeAgent => '探员';

  @override
  String get hostAuthModeInherit => '从组中继承';

  @override
  String get hostAuthTitle => '认证';

  @override
  String get hostAuthUseExisting => '使用已有的证书...';

  @override
  String get hostAuthLinkedDeleted => '链接的证书被删除';

  @override
  String hostAuthLinkedShared(String name, String kind) {
    return '$name 共享$kind';
  }

  @override
  String get hostAuthUnlink => '输入密码代替';

  @override
  String get hostAuthChangeLinked => '更改...';

  @override
  String get hostAuthPasswordSaved => '密码已保存 ';

  @override
  String get hostAuthPasswordLabel => '密码';

  @override
  String get hostAuthNewPasswordLabel => '新密码';

  @override
  String get hostAuthPasswordHelper => '连接时留空以询问。';

  @override
  String get hostAuthSavePassword => '在保险库中保存密码';

  @override
  String get hostAuthSavePasswordOn => '存储端到端加密为单独的密件。您可以在编辑连接时明确披露。';

  @override
  String get hostAuthSavePasswordOff => '没有东西被存储——终端每次连接时都会要求密码.';

  @override
  String get hostAuthKeepSaved => '保存保存的密码';

  @override
  String get hostAuthKeyLabel => '密钥';

  @override
  String get hostAuthGenerateKey => '生成 Ed25519...';

  @override
  String get hostAuthImportKey => '导入密钥...';

  @override
  String get hostAuthPassphraseRemembered => '记住密钥口令';

  @override
  String get hostAuthForget => '忘记';

  @override
  String get hostAuthPassphraseForgotten => '口令被遗忘';

  @override
  String get hostAuthKeyPassphraseLabel => '密钥口令';

  @override
  String get hostAuthKeyPassphraseHelper => '密钥会保留口令保护。在连接时没有“ 记住” 您会被询问。';

  @override
  String get hostAuthRememberPassphrase => '记住口令';

  @override
  String get hostAuthRememberPassphraseHelper => '存储为此密钥的单独加密密钥( 由其所有主机共享) 。';

  @override
  String get hostAuthAgentPathLabel => '套接字路径或管道名称';

  @override
  String hostAuthAgentPathHint(String unixPath, String windowsPath) {
    return '$unixPath 或 $windowsPath (中文(简体) ).';
  }

  @override
  String get hostAuthAgentNote => '钥匙留在经纪人中; ConsoleCrypt 永远看不到.';

  @override
  String get hostAuthInheritNoGroup => '此主机不在一个组中, 因此没有什么可以继承的: 终端会请求密码。选择一个组或另一个模式。';

  @override
  String hostAuthInheritFromGroup(String group) {
    return '用户名和证书来自组 \" $group \" 及其父母——见有效设置。';
  }

  @override
  String hostAuthPreviewPasswordShared(String name) {
    return '密码... $name';
  }

  @override
  String get hostAuthPreviewPasswordDeleted => '密码 ・ 删除证书';

  @override
  String get hostAuthPreviewPasswordSaved => '密码 ^ 保存在保险库中';

  @override
  String get hostAuthPreviewPasswordPrompt => '密码 ^ 连接时询问';

  @override
  String hostAuthPreviewSshKey(String name) {
    return 'SSH 键... $name';
  }

  @override
  String hostAuthPreviewAgentAt(String path) {
    return '代理在$path';
  }

  @override
  String get hostAuthErrorKeyRequired => '选择、生成或导入SSH 密钥';

  @override
  String get hostAuthErrorAgentPathRequired => '输入代理套接字路径或管道名称';

  @override
  String get hostAuthPickerTitle => '使用现有的证书';

  @override
  String get hostAuthPickerEmpty => '尚未有全权证书';

  @override
  String hostAuthPickerUser(String username) {
    return '用户$username';
  }

  @override
  String hostAuthPickerUsedBy(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '由$count主机使用');
    return '$_temp0';
  }

  @override
  String get hostAuthPickerNew => '新的证书...';

  @override
  String get hostPickerDefaultTitle => '连接到主机';

  @override
  String get hostPickerSearchHint => '搜索主机';

  @override
  String get hostPickerEmpty => '没有主机';

  @override
  String get jumpChainDeletedHost => '(删除的主机)';

  @override
  String get jumpChainThisDevice => '这个设备';

  @override
  String get jumpChainTarget => '目标';

  @override
  String get jumpChainEmpty => '无跳跃——在下面添加第一个跳跃主机.';

  @override
  String get jumpChainMoveUp => '向上移动';

  @override
  String get jumpChainRemoveHop => '删除跳';

  @override
  String get jumpChainAddTooltip => '添加跳转主机';

  @override
  String get jumpChainAddHop => '添加跳转';

  @override
  String get groupsTitle => '分组';

  @override
  String get groupsSubtitle => '主机从组链中继承用户名,端口,证书和跳转配置.';

  @override
  String get groupsNewGroup => '新建组';

  @override
  String get groupsEditGroup => '编辑组';

  @override
  String get groupsEmpty => '还没有团体';

  @override
  String groupsHostCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count 主机');
    return '$_temp0';
  }

  @override
  String get groupsHasDefaults => '有默认';

  @override
  String groupsDeleteTitle(String name) {
    return '删除组$name ?';
  }

  @override
  String get groupsDeleteMessage => '分组和主机会移动到父组。从此组继承设置的主机会从父组继承。';

  @override
  String get groupsAddSubgroup => '添加分组';

  @override
  String get groupsSourceOwn => '设置在此组上';

  @override
  String groupsSourceInherited(String name) {
    return '从$name继承';
  }

  @override
  String get groupsSourceUnset => '未设置';

  @override
  String get groupsUsername => '用户名';

  @override
  String get groupsPort => '端口';

  @override
  String get groupsCredential => '凭据';

  @override
  String get groupsJumpProfile => '跳转配置';

  @override
  String get groupsHostsSection => '此组中的主机';

  @override
  String get groupsNoHosts => '此组中没有直接主机。';

  @override
  String get groupsParentGroup => '家长团体';

  @override
  String get groupsNoParent => '无(最高层)';

  @override
  String get groupsDefaultsHeading => '主机继承的默认值( 留空以继承父机)';

  @override
  String get groupsInherit => '继承';

  @override
  String get groupsJumpInherit => '继承/直接';

  @override
  String get groupsJumpProfilesSection => '跳转到配置文件';

  @override
  String get groupsNewJumpProfile => '新建跳跃配置';

  @override
  String get groupsEditJumpProfile => '编辑跳转配置';

  @override
  String get groupsJumpProfilesEmpty => '可重用命令的跳转主机链。';

  @override
  String get settingsTitle => '设置';

  @override
  String get settingsRenameProfileTitle => '重命名配置文件';

  @override
  String settingsRemoveProfileTitle(String name) {
    return '从此设备中删除配置文件$name ?';
  }

  @override
  String get settingsRemoveProfileLocalMessage => '从此设备中删除此配置的本地数据库和密钥。没有备份, 数据就会永远消失。';

  @override
  String get settingsRemoveProfileSyncedMessage => '此配置文件的本地数据从此设备中删除。您服务器上的加密保险库没有被触动。';

  @override
  String get settingsRemoveProfileConfirm => '删除配置文件';

  @override
  String get settingsProfilesTitle => '配置档案';

  @override
  String get settingsProfilesSubtitle => '每个配置文件都是单独的加密保险库(如本地个人保险库和同步的工作保险库).';

  @override
  String get settingsAddProfile => '添加配置文件';

  @override
  String settingsProfileActive(String name) {
    return '$name(活跃)';
  }

  @override
  String get settingsProfileSwitch => '切换';

  @override
  String get settingsSyncTitle => '同步';

  @override
  String get settingsSyncLocalSubtitle => '仅本地: 没有服务器, 没有账户。';

  @override
  String get settingsSyncLocalBody => '上传此保险库端到端加密到您自己托管的服务器, 用于其它设备。';

  @override
  String get settingsEnableSync => '启用同步...';

  @override
  String get settingsAccountSyncTitle => '账户同步( S)';

  @override
  String get settingsServerLabel => '服务器';

  @override
  String get settingsAccountLabel => '账户';

  @override
  String get settingsThisDeviceLabel => '这个设备';

  @override
  String get settingsChangeAccountPassword => '更改账户密码';

  @override
  String get settingsDisconnect => '断开连接( 在本地保存数据)...';

  @override
  String get settingsSignOut => '退出登录';

  @override
  String get settingsDifferentServerHint => '要使用不同的服务器,请添加另一个配置文件;每个配置文件都有自己的服务器和保险库.';

  @override
  String get settingsVaultTitle => '保险库';

  @override
  String get settingsVaultNameLabel => '名称';

  @override
  String get settingsVaultIdLabel => '默认标识';

  @override
  String get settingsAutoLockLabel => '自动锁定';

  @override
  String settingsAutoLockAfterMinutes(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count分钟后', one: '1分钟后');
    return '$_temp0';
  }

  @override
  String settingsAutoLockAfterHours(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '之后 $count 小时', one: '1小时后');
    return '$_temp0';
  }

  @override
  String get settingsAutoLockNever => '从不';

  @override
  String get settingsChangePassphrase => '更改口令...';

  @override
  String get settingsNewRecoveryKit => '新建复原套件...';

  @override
  String get settingsBackups => '备份...';

  @override
  String get settingsLockNow => '现在锁定';

  @override
  String get settingsAiProvidersTitle => 'AI 提供商';

  @override
  String get settingsAiProvidersSubtitle => 'AI可以读取您允许的片段和主机元数据——从密码,密钥或保险库密钥都不要.';

  @override
  String settingsAiProviderDefault(String name) {
    return '$name 违约';
  }

  @override
  String get settingsAiProviderApiKeyStored => 'API 存储的密钥';

  @override
  String get settingsDefaultPrivacyLabel => '默认隐私';

  @override
  String get settingsKeepAiConversations => '在保险库里保持人工智能对讲';

  @override
  String get settingsKeepAiConversationsHelp => '存储加密( 并同步用于同步配置文件) 。关闭: 对话被遗忘。';

  @override
  String get settingsTerminalTitle => '终端';

  @override
  String get settingsCommandHistoryLabel => '命令历史';

  @override
  String settingsHistoryPendingSync(String description) {
    return '$description (同步启用后产生效果)';
  }

  @override
  String get settingsFontSizeLabel => '字体大小';

  @override
  String get settingsScrollbackLabel => '回滚';

  @override
  String settingsScrollbackLines(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count线路', one: '$count线');
    return '$_temp0';
  }

  @override
  String get settingsTerminalBuffersNote => '终端缓冲器会留在此设备上, 并且永远不会同步。';

  @override
  String get settingsAppearanceSecurityTitle => '外观和安全';

  @override
  String get settingsThemeLabel => '主题';

  @override
  String get settingsThemeSystem => '系统';

  @override
  String get settingsThemeLight => '浅色';

  @override
  String get settingsThemeDark => '深色';

  @override
  String get settingsClearClipboardLabel => '清除剪贴板';

  @override
  String settingsClearClipboardAfter(int seconds) {
    return '$seconds s 在复制一个秘密后';
  }

  @override
  String get settingsKnownHostsTitle => '已知主机';

  @override
  String get settingsKnownHostsSubtitle => '您信任的主机密钥。更改的密钥总是会阻断连接, 直到您删除这里的旧密钥。';

  @override
  String get settingsKnownHostsEmpty => '还没有已知的主机。';

  @override
  String settingsRemoveKnownHostTitle(String host) {
    return '删除$host?';
  }

  @override
  String get settingsRemoveKnownHostMessage => '下一个连接将要求您再次验证主机密钥。';

  @override
  String get settingsDialogPassphraseChanged => '默认口令已更改';

  @override
  String get settingsDialogChangePassphraseTitle => '更改保险库口令';

  @override
  String get settingsDialogCurrentPassphrase => '当前口令';

  @override
  String get settingsDialogNewPassphrase => '新建口令';

  @override
  String get settingsDialogRepeatPassphrase => '重复新口令';

  @override
  String get settingsDialogChangePassphraseConfirm => '更改口令';

  @override
  String get settingsDialogNewKitTitle => '新建回收套件';

  @override
  String get settingsDialogNewKitWarning => '新的回收密钥将取代当前 : 您的旧回收 Kit 停止工作。在关闭此窗口前保存新套件。';

  @override
  String get settingsDialogShowKit => '显示回收套件';

  @override
  String get settingsDialogKitSaved => '我留着';

  @override
  String get settingsDialogGenerateKit => '生成新工具包';

  @override
  String get settingsDialogAccountPasswordChanged => '账户密码已更改';

  @override
  String get settingsDialogAccountPasswordTitle => '更改账户密码';

  @override
  String get settingsDialogCurrentPassword => '当前密码';

  @override
  String get settingsDialogNewPassword => '新密码';

  @override
  String settingsDialogPasswordMinLength(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: ' 至少$count 字符', one: ' 至少一个字符');
    return '$_temp0';
  }

  @override
  String get settingsDialogVaultPassphraseUnchanged => '这不会改变您的保险库口令。';

  @override
  String get aiProviderDialogAddTitle => '添加 AI 提供者';

  @override
  String aiProviderDialogEditTitle(String name) {
    return '编辑 $name';
  }

  @override
  String get aiProviderDialogProviderLabel => '供应商';

  @override
  String get aiProviderDialogNameLabel => '名称';

  @override
  String get aiProviderDialogBaseUrlLabel => '基础 URL';

  @override
  String get aiProviderDialogInsecureHttp => 'Plain http to a远程主机: API 密钥和提示会不加密。使用 https。';

  @override
  String get aiProviderDialogChatModelLabel => '聊天模式';

  @override
  String get aiProviderDialogEmbeddingModelLabel => '嵌入模型( 可选)';

  @override
  String get aiProviderDialogApiKeyLabel => 'API 键';

  @override
  String get aiProviderDialogApiKeyStoredHint => '存储一个密钥—— 替换它的类型';

  @override
  String get aiProviderDialogApiKeyOptionalHint => '本地供应商可选';

  @override
  String get aiProviderDialogApiKeyHelp => '存储为保险库中的加密密件。 AI 子系统无法读取它; 它只被放入请求的HTTP 标题。';

  @override
  String get aiProviderDialogRemoveApiKey => '删除存储的API 密钥';

  @override
  String get aiProviderDialogPrivacyProfileLabel => '隐私简介';

  @override
  String aiProviderDialogLocalProfileWarning(String profile) {
    return '$profile profile 发送更多上下文。仅使用您控制的机器上运行的模型。';
  }

  @override
  String get aiProviderDialogTimeoutLabel => '超时(s)';

  @override
  String get aiProviderDialogStreaming => '流线';

  @override
  String get aiProviderDialogToolCalling => '工具调用';

  @override
  String get aiProviderDialogDefault => '默认';

  @override
  String get aiProviderDialogHealthOk => '通过连接测试';

  @override
  String get aiProviderDialogHealthFailed => '连接测试失败';

  @override
  String aiProviderDialogHealthModels(String models) {
    return '型号:$models';
  }

  @override
  String get aiProviderDialogSaveAndTest => '保存测试( T)';

  @override
  String get runFlowVariablesPreview => '预览';

  @override
  String get runFlowConfirmTitle => '运行命令?';

  @override
  String runFlowLocalRules(String reasons) {
    return '当地规则:$reasons.';
  }

  @override
  String runFlowDeclaredVsLocal(String declared, String effective) {
    return '宣布为$declared;当地规则评定为$effective.';
  }

  @override
  String runFlowAckDestructive(String host) {
    return '我知道这个命令可以删除或销毁$host上的数据';
  }

  @override
  String runFlowAckModifying(String host) {
    return '我知道这个命令改变了$host状态';
  }

  @override
  String get runFlowAckUnknown => '我审查了这个命令,地方规则不知道它的效果';

  @override
  String get runFlowRun => '运行';

  @override
  String get runFlowPickHostTitle => '运行在哪个主机?';

  @override
  String get runFlowOpenTerminalFirst => '先打开终端标签。';

  @override
  String get snippetEditorTitleNew => '新片断';

  @override
  String get snippetEditorTitleEdit => '编辑片断';

  @override
  String get snippetEditorAiDraftBanner => '由AI起草,在保存前审查命令及其风险.';

  @override
  String get snippetEditorTemplateLabel => '命令模板';

  @override
  String get snippetEditorShellLabel => 'shell / 方言( 可选)';

  @override
  String get snippetEditorVariables => '变量';

  @override
  String get snippetEditorVariableDefault => '默认';

  @override
  String get snippetEditorRisk => '风险';

  @override
  String get snippetEditorEffective => '有效的: ';

  @override
  String snippetEditorLocalRules(String detail) {
    return '当地规则:$detail';
  }

  @override
  String get snippetPickerTitle => '运行片段';

  @override
  String get snippetPickerSearchHint => '搜索片断';

  @override
  String get snippetsTypeLabel => '类型';

  @override
  String snippetsSubtitle(String syntax) {
    return '可重用命令$syntax 。运行总是首先显示命令、主机和风险。';
  }

  @override
  String get snippetsNewButton => '新片断';

  @override
  String get snippetsSearchHint => '搜索文本或意图, 如“ 磁盘空间 ”';

  @override
  String get snippetsAllTypes => '各类';

  @override
  String get snippetsEmptyTitle => '还没有片段';

  @override
  String get snippetsEmptyMessage => '保存您经常使用的命令—— 或者让AI起草它们.';

  @override
  String get snippetsNothingFound => '没找到';

  @override
  String get snippetsDraftedByAi => '大赦国际起草';

  @override
  String get snippetsSemanticMatch => '语义匹配';

  @override
  String snippetsUsedCount(int count) {
    return '已使用$count×';
  }

  @override
  String get snippetsInsertIntoTerminal => '插入终端';

  @override
  String get snippetsRun => '运行';

  @override
  String snippetsDeleteTitle(String name) {
    return '删除$name?';
  }

  @override
  String get snippetsDeleteMessage => '片段从这个保险库里取出.';

  @override
  String get aiChatSubtitle => '回答从不自行运行: 插入或运行... 总是先问你.';

  @override
  String get aiChatProviderLabel => '供应商';

  @override
  String get aiChatNewConversation => '新对话';

  @override
  String aiChatRemoteProviderNotice(String profile, String description) {
    return '远程提供者 ^ 隐私简介$profile : $description. 密码和密钥从未发送.';
  }

  @override
  String get aiChatLocalModelNotice => '本地模特儿 机器一无所有';

  @override
  String get aiChatNoProviderBanner => '没有配置 AI 提供者。';

  @override
  String get aiChatEmptyTitle => '询问您的服务器';

  @override
  String get aiChatEmptyMessage => '例如\"在/var/log中找到最大的文件\"或\"安全地重新开始nginx\".';

  @override
  String get aiChatInputHint => '信件( 要发送, Shift+ Enter for a new line)';

  @override
  String get aiChatStop => '停下来';

  @override
  String get aiChatSend => '发送';

  @override
  String get aiChatIncludeSelection => '包含选定的终端文本';

  @override
  String get aiChatSelectToInclude => '在终端中选择要包含的文本';

  @override
  String get aiChatErrorNoProvider => '没有配置 AI 提供者。在设置 ~ AI 提供者中添加一个。';

  @override
  String get aiChatErrorRequestFailed => 'AI请求失败.';

  @override
  String get aiChatStoppedSuffix => '...(停止)';

  @override
  String paletteGenerateCommand(String query) {
    return '生成命令: \" $query \"';
  }

  @override
  String get paletteGenerateSubtitle => '询问 AI( 从未自动运行 )';

  @override
  String get paletteScreenSnip => '截取屏幕';

  @override
  String get paletteSnipFailed => '无法打开截图工具。';

  @override
  String get paletteNewHost => '新建主机';

  @override
  String get paletteOpenAiChat => '打开 AI 聊天';

  @override
  String get paletteLockVault => '锁定保险库';

  @override
  String get paletteSearchHint => '按名称或IP、片段、动作或描述命令查找主机...';

  @override
  String get paletteAskSelectionHint => '询问所选终端输出...';

  @override
  String get paletteAiSuggestion => '大赦国际的建议';

  @override
  String get paletteThinking => '想着...';

  @override
  String get paletteErrorNoProvider => '没有配置 AI 提供者。添加Ollama, LM Studio 或 DeepSeek 在设置中。';

  @override
  String paletteRiskMismatch(String aiRisk, String localRisk) {
    return '大赦国际对此进行了评级$aiRisk;地方规则说$localRisk.';
  }

  @override
  String get codeBlocksInsert => '插入';

  @override
  String get codeBlocksRun => '运行...';

  @override
  String get codeBlocksSaveAsSnippet => '另存为片段';

  @override
  String codeBlocksSnippetSaved(String name) {
    return '片段 \" $name \" 已保存';
  }

  @override
  String codeBlocksProcessedLocally(String profile) {
    return '由当地模式处理 ^ $profile 剖面图';
  }

  @override
  String codeBlocksSanitized(int count, String profile) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$profile 离开该装置前的卫生$count',
      one: ' 在离开此装置之前, 已进行消毒$profile 。 $count 物品已校正',
    );
    return '$_temp0';
  }

  @override
  String get terminalEmptyTitle => '无公开会议';

  @override
  String terminalEmptyMessage(String shortcut) {
    return '连接到一个主机打开终端标签$shortcut.';
  }

  @override
  String get terminalConnectToHost => '连接到主机';

  @override
  String get terminalCloseTab => '关闭标签页';

  @override
  String terminalNewTabTooltip(String shortcut) {
    return '新标签 $shortcut )';
  }

  @override
  String get terminalSplit => '分开';

  @override
  String get terminalSplitRight => '向右分割';

  @override
  String get terminalSplitDown => '分开';

  @override
  String get terminalNextPane => '下一个窗格';

  @override
  String get terminalClosePane => '关闭窗格';

  @override
  String terminalSplitFull(int count) {
    return '标签最多$count 面板';
  }

  @override
  String get terminalSnippets => '代码片段';

  @override
  String get terminalAskAi => '问大赦国际';

  @override
  String terminalHostKeyChangedTitle(String host) {
    return '$host号)';
  }

  @override
  String terminalHostKeyChangedMessage(String keyType, String fingerprint) {
    return '主机密钥校验失败: 服务器密钥 CHANGED。这可以指中层人物攻击。连接被拒绝。\n已显示 :$keyType$fingerprint 。如果服务器真的被重新安装, 请移除设置 ~ 已知主机中的旧密钥并重新连接。';
  }

  @override
  String terminalUnknownHostTitle(String host) {
    return '未知主机$host';
  }

  @override
  String get terminalUnknownHostMessage => '此主机的真实性无法确定。在信任它之前, 请先与服务器管理员核对指纹。';

  @override
  String get terminalHostKeyReject => '拒绝';

  @override
  String get terminalHostKeyAcceptOnce => '接受一次';

  @override
  String get terminalHostKeyAcceptAndSave => '接受并保存';

  @override
  String get terminalConnecting => '正在连接';

  @override
  String terminalConnectingRoute(String route) {
    return '连接$route';
  }

  @override
  String get terminalReconnecting => '重新连接';

  @override
  String terminalReconnectingRoute(String route) {
    return '重新连接$route';
  }

  @override
  String get terminalSessionEnded => '会议结束';

  @override
  String get terminalDisconnected => '已断开连接';

  @override
  String get terminalConnectionClosed => '连接被关闭。';

  @override
  String get terminalReconnect => '重新连接';

  @override
  String terminalKeyPassphrasePromptTitle(String key, String host) {
    return '$host上键“$key”的口令';
  }

  @override
  String get terminalKeyPassphraseRetry => '错误的密钥口令, 请再试一次。';

  @override
  String get terminalKeyPassphraseLabel => '密钥口令';

  @override
  String get terminalKeyPassphraseHelper => '仅用于此连接 – 没有保存。用密钥证书保存以跳过此步骤。';

  @override
  String terminalPasswordPromptTitle(String user, String host) {
    return '$user@$host的密码';
  }

  @override
  String get terminalPasswordRetry => '拒绝许可 请再试一次';

  @override
  String get terminalPasswordLabel => '密码';

  @override
  String get terminalPasswordHelper => '仅用于此连接—— 没有保存。在主机设置中保存以跳过此步骤。';

  @override
  String get sftpOpenPickerTitle => '打开SFTP';

  @override
  String get sftpSubtitle => '在SSH上浏览和传输文件.';

  @override
  String get sftpDisconnect => '断开连接';

  @override
  String get sftpNotConnectedTitle => '未连接';

  @override
  String get sftpNotConnectedMessage => '选择一个主机浏览、传输和编辑其文件。';

  @override
  String get sftpLocalPaneTitle => '这个设备';

  @override
  String get sftpUploadTooltip => '上传选中的文件';

  @override
  String get sftpDownloadTooltip => '下载选中的文件';

  @override
  String get sftpNewFolder => '新建文件夹';

  @override
  String sftpDeleteTitle(String name) {
    return '删除$name?';
  }

  @override
  String get sftpDeleteFolderMessage => '文件夹和里面的所有东西都在服务器上被删除。';

  @override
  String get sftpDeleteFileMessage => '文件在服务器上被删除。';

  @override
  String get sftpRefresh => '刷新';

  @override
  String get sftpUp => '上来';

  @override
  String sftpTransferDone(String size) {
    return '完成$size';
  }

  @override
  String sftpTransferFailed(String detail) {
    return '失败:$detail';
  }

  @override
  String get sftpTransferCancelled => '已取消';

  @override
  String sftpTransferProgress(String done, String total) {
    return '$done,第$total条';
  }

  @override
  String sftpTransferSpeed(String size) {
    return '$size个)/s';
  }

  @override
  String get sftpToolbarQuickLook => '快来看';

  @override
  String get sftpToolbarActions => '行动';

  @override
  String get sftpToolbarTransfers => '转账';

  @override
  String get sftpToolbarEditing => '编辑';

  @override
  String get sftpToolbarShowLocal => '并肩显示本地文件';

  @override
  String get sftpToolbarHideLocal => '隐藏本地文件';

  @override
  String get sftpToolbarSwitchHost => '连接到另一个主机';

  @override
  String get sftpSearchHint => '搜索此文件夹';

  @override
  String get sftpSearchClear => '清除搜索';

  @override
  String sftpConnecting(String host) {
    return '连接$host.';
  }

  @override
  String get sftpPathHint => '键入路径, 例如 / var/ www 或 ~/ logs';

  @override
  String sftpPathEditTooltip(String shortcut) {
    return '点击或按下$shortcut 键入路径';
  }

  @override
  String get sftpCopyPath => '复制路径';

  @override
  String get sftpCopyWhatPath => '路径';

  @override
  String get sftpCopyWhatName => '名称';

  @override
  String get sftpColumnName => '名称';

  @override
  String get sftpColumnSize => '大小';

  @override
  String get sftpColumnKind => '温柔';

  @override
  String get sftpColumnModified => '修改日期';

  @override
  String get sftpColumnPermissions => '权限';

  @override
  String get sftpColumnOwner => '拥有者';

  @override
  String get sftpColumnGroup => '组';

  @override
  String get sftpFoldersFirst => '文件夹优先';

  @override
  String get sftpShowHidden => '显示隐藏文件';

  @override
  String get sftpResetColumns => '重设列';

  @override
  String get sftpExpand => '扩展';

  @override
  String get sftpCollapse => '折叠';

  @override
  String get sftpEmptyFolder => '此文件夹为空';

  @override
  String sftpNoMatches(String query) {
    return '此文件夹中没有匹配“ $query ” 的内容。';
  }

  @override
  String sftpSymlinkTooltip(String target) {
    return '符号链接_$target';
  }

  @override
  String get sftpDanglingLink => '断开链接';

  @override
  String get sftpKindFolder => '文件夹';

  @override
  String get sftpKindText => '纯文本';

  @override
  String get sftpKindDocument => '文档';

  @override
  String sftpKindDocumentFormat(String format) {
    return '$format号文件';
  }

  @override
  String get sftpKindExecutable => '可执行文件';

  @override
  String get sftpKindShellScript => '贝壳脚本';

  @override
  String sftpKindImage(String format) {
    return '$format图像';
  }

  @override
  String sftpKindArchive(String format) {
    return '$format 存档';
  }

  @override
  String get sftpKindLog => '日志文件';

  @override
  String get sftpKindConfig => '配置';

  @override
  String get sftpKindPdf => 'PDF 文档';

  @override
  String get sftpKindAudio => '音频';

  @override
  String get sftpKindVideo => '视频';

  @override
  String get sftpKindFont => '字体';

  @override
  String get sftpKindKey => '密钥或证书';

  @override
  String get sftpKindDatabase => '数据库';

  @override
  String get sftpKindSymlink => '符号链接';

  @override
  String sftpKindSymlinkTo(String kind) {
    return '链接-$kind';
  }

  @override
  String get sftpKindSpecial => '特殊文件';

  @override
  String sftpStatusFolders(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count文件夹', one: '$count文件夹');
    return '$_temp0';
  }

  @override
  String sftpStatusFiles(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count文件', one: '$count 文件');
    return '$_temp0';
  }

  @override
  String sftpStatusSummary(String folders, String files, String size) {
    return '$folders, $files, $size';
  }

  @override
  String sftpStatusSelection(String selected, String total, String size) {
    return '$selected,其中$total选定,$size';
  }

  @override
  String sftpStatusEditing(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '编辑$count 文件', one: '编辑$count 文件');
    return '$_temp0';
  }

  @override
  String get sftpStatusSecure => '加密SFTP连接于SSH';

  @override
  String get sftpApplications => '应用程序';

  @override
  String get sftpChooseApplication => '选择应用程序';

  @override
  String errorEditorLaunchFailed(String detail) {
    return '无法打开编辑器。使用 Open With 选择另一个应用程序。系统消息 :$detail';
  }

  @override
  String errorApplicationPickerFailed(String detail) {
    return '无法显示程序拾取器。系统消息 :$detail';
  }

  @override
  String get sftpActionOpenWith => '打开方式...';

  @override
  String get sftpActionDownload => '下载...';

  @override
  String get sftpActionUploadHere => '上传到这里...';

  @override
  String get sftpActionNewFile => '新文件';

  @override
  String get sftpActionDuplicate => '复制';

  @override
  String get sftpActionGetInfo => '获取信息';

  @override
  String get sftpActionCopyName => '复制名称';

  @override
  String get sftpUntitledFolder => '无标题文件夹';

  @override
  String get sftpUntitledFile => '无标题.txt';

  @override
  String get sftpDuplicateSuffix => '复制';

  @override
  String get sftpNameInvalid => '名字不能是空的或包含 \" / \" 。';

  @override
  String sftpDeleteManyTitle(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: ' 删除$count项?', one: ' 删除$count项?');
    return '$_temp0';
  }

  @override
  String get sftpDeleteManyMessage => '选中的项目在服务器上被删除, 包含全部内容的文件夹。这不能被撤销。';

  @override
  String sftpUploadStarted(int count, String folder) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '上传$count项至$folder',
      one: '上传$count项至$folder',
    );
    return '$_temp0';
  }

  @override
  String sftpDownloadStarted(int count, String folder) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '下载$count项至$folder',
      one: '下载$count项至$folder',
    );
    return '$_temp0';
  }

  @override
  String sftpMoved(int count, String folder) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '将$count物品移至$folder',
      one: '将$count项移至$folder项',
    );
    return '$_temp0';
  }

  @override
  String sftpDragItems(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count项', one: '$count 项目');
    return '$_temp0';
  }

  @override
  String get sftpDisconnectEditsTitle => '停止编辑和断开?';

  @override
  String sftpDisconnectEditsMessage(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count来自这个主机的文件正在编辑中.',
      one: '$count 来自此主机的文件正在编辑中.',
    );
    return '$_temp0 等待更改的,先上传;如不成功,则保留工作副本以收回。';
  }

  @override
  String sftpInfoTitle(String name) {
    return '“$name......”资料';
  }

  @override
  String get sftpInfoWhere => '何处';

  @override
  String sftpInfoSizeBytes(String size, String bytes) {
    return '$size $bytes 字节';
  }

  @override
  String get sftpInfoLinkTarget => '指点';

  @override
  String get sftpInfoSymlinkNote => '符号链接的权限适用于它指向的项目.';

  @override
  String get sftpInfoOthers => '其他人员';

  @override
  String get sftpInfoRead => '读取';

  @override
  String get sftpInfoWrite => '写入';

  @override
  String get sftpInfoExecute => '执行';

  @override
  String get sftpInfoOctal => '八面体';

  @override
  String get sftpInfoOctalInvalid => '输入 0 到 7 的 3 或 4 位数。';

  @override
  String get sftpInfoApply => '应用';

  @override
  String get sftpInfoPermissionsSaved => '权限更改。';

  @override
  String get sftpQuickLookFolder => '文件夹没有预览。';

  @override
  String get sftpQuickLookNoPreview => '此类文件没有预览。';

  @override
  String sftpQuickLookTooLarge(String size) {
    return '这个图像太大,无法预览$size.';
  }

  @override
  String get sftpQuickLookImageError => '图像无法显示。';

  @override
  String sftpQuickLookTruncated(String shown, String total) {
    return '显示$total的第一$shown.';
  }

  @override
  String get sftpQuickLookMemoryNote => '只装入内存中——此设备上没有保存任何内容.';

  @override
  String get sftpQuickLookOpenInEditor => '在编辑器中打开';

  @override
  String get sftpActivityHide => '隐藏面板';

  @override
  String get sftpTransfersClear => '清除完成';

  @override
  String get sftpTransfersEmpty => '尚未转移。将文件丢入列表, 或使用上传...';

  @override
  String get sftpTransferDirectionUpload => '上传';

  @override
  String get sftpTransferDirectionDownload => '下载';

  @override
  String get sftpTransferQueued => '等着...';

  @override
  String sftpTransferTo(String path) {
    return '改为$path';
  }

  @override
  String get sftpEditingEmpty => '没有正在编辑文件。在默认的应用程序中双击一个文件打开它—— 每个保存都会自动上传。';

  @override
  String get sftpEditStatusOpening => '打开...';

  @override
  String get sftpEditStatusSynced => '同步';

  @override
  String get sftpEditStatusModified => '已修改';

  @override
  String sftpEditStatusUploading(int percent) {
    return '上传$percent%';
  }

  @override
  String get sftpEditStatusUploadingUnknown => '正在上传...';

  @override
  String get sftpEditStatusConflict => '冲突';

  @override
  String get sftpEditStatusError => '上传失败';

  @override
  String get sftpEditStatusClosed => '关闭';

  @override
  String sftpEditLastSynced(String time) {
    return '上传$time';
  }

  @override
  String sftpEditOpenedWith(String app) {
    return '在$app年)中';
  }

  @override
  String get sftpEditRevealFinder => '在查找器中显示';

  @override
  String get sftpEditRevealExplorer => '在探索器中显示';

  @override
  String get sftpEditRevealOther => '在文件夹中显示';

  @override
  String get sftpEditReopen => '在编辑器中重新打开';

  @override
  String get sftpEditSyncNow => '现在同步';

  @override
  String get sftpEditStop => '停止编辑';

  @override
  String get sftpEditResolve => '决断...';

  @override
  String sftpEditOpened(String name) {
    return '打开“ $name ” 。每个保存都会自动上传。';
  }

  @override
  String sftpEditStopped(String name) {
    return '停止编辑“$name......”。';
  }

  @override
  String sftpEditStopFailed(String name, String detail) {
    return '“$name”没有上传$detail,编辑继续。';
  }

  @override
  String sftpEditTooLarge(String name, String size, String limit) {
    return '“$name”太大,无法编辑$size,限制$limit。';
  }

  @override
  String get sftpEditNotAFile => '只能打开正则文件进行编辑.';

  @override
  String get sftpEditHintTitle => '在另一个应用程序中编辑';

  @override
  String sftpEditHintBody(String name) {
    return '“ $name ” 在其默认的app中打开, 外部ConsoleCrypt。在您编辑时, 此设备上保留了一个私有的工作副本( 只能由您的用户读取), 而每个保存都会自动上传到服务器。';
  }

  @override
  String get sftpEditHintControl => 'ConsoleCrypt 无法控制该应用程序对文件所做的一切 — — 例如它自己的自动保存、备份或云同步。当您停止编辑时, 工作副本将被删除。';

  @override
  String get sftpEditHintDontShow => '不要再出现';

  @override
  String sftpConflictTitle(String name) {
    return '“$name”在服务器上更改';
  }

  @override
  String get sftpConflictMessage => '打开后有人在服务器上更改了此文件。您最近的保存尚未上传, 因此没有被覆盖。选择要做什么 :';

  @override
  String get sftpConflictDeletedMessage => '文件在打开后被删除或移动到服务器上。您最近的保存尚未上传。';

  @override
  String sftpConflictRemoteMeta(String size, String time) {
    return '服务器版本:$size,修改版$time';
  }

  @override
  String get sftpConflictOverwriteTitle => '覆盖服务器文件';

  @override
  String get sftpConflictOverwriteBody => '上传您的版本。服务器上的其他更改已丢失。';

  @override
  String get sftpConflictRecreateTitle => '再次上传我的版本';

  @override
  String get sftpConflictRecreateBody => '从本地副本再次创建服务器上的文件。';

  @override
  String get sftpConflictKeepTitle => '都留着';

  @override
  String sftpConflictKeepBody(String name) {
    return '将副本旁边的服务器版本保存为“ $name.remote-...), 并打开以进行比较。您的下一个保存上传您的版本。';
  }

  @override
  String get sftpConflictDiscardTitle => '丢弃我的修改';

  @override
  String get sftpConflictDiscardBody => '用服务器版本替换本地副本。';

  @override
  String get sftpConflictLater => '以后决定';

  @override
  String get sftpLeftoversTitle => '恢复未保存的编辑吗 ?';

  @override
  String get sftpLeftoversMessage => 'ConsoleCrypt 这些远程文件正在编辑时被关闭, 其工作副本仍在此设备上。';

  @override
  String get sftpLeftoverModified => '本地更改, 尚未在服务器上更改';

  @override
  String get sftpLeftoverUnchanged => '无本地更改';

  @override
  String get sftpLeftoverUnknownHost => '未知的主机 - 只能删除';

  @override
  String get sftpLeftoverUnreadable => '会话数据损坏——只能删除';

  @override
  String get sftpLeftoversRecover => '已选中回收';

  @override
  String get sftpLeftoversRecoverHint => '选中的文件在冲突检查后上传, 并保持打开供编辑。其他工作副本将被删除。';

  @override
  String get sftpLeftoversDiscardAll => '删除全部';

  @override
  String get sftpLeftoversLater => '回头见';

  @override
  String sftpLeftoversRecovered(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '已恢复$count 文件', one: '已恢复$count 文件');
    return '$_temp0';
  }

  @override
  String sftpLeftoversFailed(String name, String error) {
    return '无法恢复 \" $name \" : $error';
  }

  @override
  String get sftpDebugMenu => '模拟( 演示后端)';

  @override
  String get sftpDebugSave => '在编辑器中模拟保存';

  @override
  String get sftpDebugRemoteChange => '模拟服务器上的更改';

  @override
  String get sftpDebugFailUpload => '使下一次上传失败';

  @override
  String get sftpDebugFailTransfer => '使下次传输失败( demo)';

  @override
  String get tunnelsTitle => '隧道';

  @override
  String get tunnelsSubtitle => '端口通过 SSH 转发。隧道在此设备上运行; 它们的定义与您的保险库同步。';

  @override
  String get tunnelsNew => '新建隧道';

  @override
  String get tunnelsEmptyTitle => '还没有隧道';

  @override
  String get tunnelsEmptyMessage => '例如: 127.0.0.1:15432 → db. 内部:5432通过一个堡垒.';

  @override
  String get tunnelsDeletedHost => '(删除的主机)';

  @override
  String tunnelsStatusRunning(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '运行 ^ $count conn.');
    return '$_temp0';
  }

  @override
  String tunnelsStatusRunningSince(int count, String ago) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: ' 自$ago起运行$count conn.');
    return '$_temp0';
  }

  @override
  String get tunnelsStatusStarting => '开始...';

  @override
  String get tunnelsStatusFailed => '失败';

  @override
  String tunnelsStatusFailedDetail(String detail) {
    return '失败:$detail';
  }

  @override
  String get tunnelsStatusStopped => '已停止';

  @override
  String tunnelsPublicBindTooltip(String address) {
    return '以$address为界:网络可达';
  }

  @override
  String tunnelsSummaryRemote(String bind, String target) {
    return '远程$bind −$target';
  }

  @override
  String tunnelsTileSubtitle(String summary, String host) {
    return '$summary -通过$host';
  }

  @override
  String tunnelsDeleteTitle(String name) {
    return '删除隧道$name ?';
  }

  @override
  String get tunnelsDeleteMessage => '隧道停运,从保险库取出.';

  @override
  String get tunnelsErrorChooseHost => '选择带隧道的主机';

  @override
  String get tunnelsEdit => '编辑隧道';

  @override
  String get tunnelsHostLabel => '通过主机SSH 连接包括它的跳链)';

  @override
  String get tunnelsBindAddressRemote => '在服务器上绑定地址';

  @override
  String get tunnelsBindAddressLocal => '绑定地址( 此设备)';

  @override
  String get tunnelsPortLabel => '端口';

  @override
  String get tunnelsTargetRemote => '此设备的目标';

  @override
  String get tunnelsTargetLocal => '目标主机( 从服务器上看到)';

  @override
  String get tunnelsPublicBindTitle => '不必然回转';

  @override
  String tunnelsPublicBindWarningRemote(String address) {
    return '\"$address\"将转发的端口暴露在服务器的网络上(需要GatewayPorts),任何能够到达端口的人都可以使用隧道.';
  }

  @override
  String tunnelsPublicBindWarningLocal(String address) {
    return '\"$address\"将隧道暴露在您的本地网络中,任何能够到达此设备的人都可以使用它。使用127.0.0.1,除非您真的要使用它。';
  }

  @override
  String get tunnelsPublicBindAck => '我知道这个隧道可以从网络上到达';

  @override
  String get tunnelsAutoStart => '解锁后自动启动';

  @override
  String get approvalWaitTitle => '批准此设备';

  @override
  String approvalWaitSubtitle(String device) {
    return '在解锁ConsoleCrypt的设备上,打开设备,并审查 \" $device \" 的请求。显示的代码必须与这个代码完全一致。';
  }

  @override
  String get approvalWaitSubtitleUnnamed => '在解锁ConsoleCrypt 的设备上, 打开设备并审查“ 此设备” 的请求。显示的代码必须完全匹配。';

  @override
  String get approvalWaitCodeLabel => '此设备的校验代码';

  @override
  String get approvalWaitWaiting => '等待批准 请求将在24小时内到期';

  @override
  String get approvalWaitMismatchWarning => '如果代码不同, 请拒绝其他设备上的请求。错配可能意味着某人( 甚至一个已损坏的服务器) 正在试图添加自己的设备。';

  @override
  String get approvalWaitSimulateApproval => '模拟批准(模拟)';

  @override
  String get createVaultDefaultName => '个人';

  @override
  String get createVaultTitle => '创建保险库口令';

  @override
  String get createVaultSubtitleLocal => '它加密了这个设备中的所有文件。';

  @override
  String get createVaultSubtitleSynced => '它在同步之前加密您的保险库。您的服务器从未看到它, 它与您的账户密码不同。';

  @override
  String get createVaultNameLabel => '默认名称';

  @override
  String get createVaultPassphraseLabel => '保险库口令';

  @override
  String get createVaultRepeatPassphraseLabel => '重复口令';

  @override
  String get createVaultPassphraseMismatch => '口令不匹配';

  @override
  String get createVaultNoResetNotice => '任何人都无法为你重置此口令，包括服务器管理员。下一步将生成恢复套件，这是打开保险库的唯一备用方式。';

  @override
  String get createVaultSubmit => '创建保险库';

  @override
  String get recoveryKitTitle => '保存您的复原套件';

  @override
  String get recoveryKitSubtitle => '如果您忘记了口令, 并失去了对您信任的设备的访问权限, 这个工具箱是返回您保险库的唯一方法。请保持它离线: 打印或写下来。';

  @override
  String get recoveryKitLostFromMemory => '回收套件不再存在内存( 应用程序已重新启动) 。生成一个新的套件 —— 前一个套件停止工作。';

  @override
  String get recoveryKitGenerateNew => '生成一个新的回收套件';

  @override
  String get recoveryKitCopyWords => '复制单词';

  @override
  String get recoveryKitPrint => '打印/ 另存为 PDF';

  @override
  String get recoveryKitPrintHint => '保存的 PDF 和打印的表一样秘密——不要在云色文件夹中出现.';

  @override
  String get recoveryKitPrintFailed => '无法打印恢复套件,将文字写下来';

  @override
  String get recoveryKitSheetTitle => 'ConsoleCrypt 回收箱';

  @override
  String get recoveryKitSheetWarning => '任何有这张纸的人都可以打开你的保险库。保持它离线, 在安全的地方: 如果你忘记密码, 失去你信任的设备, 这是唯一的回路。';

  @override
  String get recoveryKitSavedConfirmation => '我把我的恢复套件藏在安全的地方';

  @override
  String get recoveryKitRevealFirst => '先揭开箱子';

  @override
  String get recoveryKitKeyNotStored => '恢复密钥从未由ConsoleCrypt或您的服务器存储——只有它能打开的信封.';

  @override
  String get recoveryKitVaultId => '默认标识';

  @override
  String get recoveryKitServer => '服务器';

  @override
  String get recoveryKitNoServerCopy => '本地配置文件——没有服务器副本';

  @override
  String get recoveryKitCreated => '已创建';

  @override
  String get recoveryKitPrivacyHint => '确保没有人能看到你的屏幕。';

  @override
  String get recoveryKitShow => '显示回收套件';

  @override
  String get verifyKitTitle => '确认恢复套件';

  @override
  String get verifyKitNoKit => '内存里没有复原套件, 回去生成一个。';

  @override
  String get verifyKitBackToRecoveryKit => '回复原套件';

  @override
  String get verifyKitSubtitle => '输入套件中请求的单词以证明已保存。需要此步骤。';

  @override
  String verifyKitWordLabel(int index) {
    return '词 # $index';
  }

  @override
  String get verifyKitMismatch => '那些词和你的复原套不匹配 检查一下你的套件再试一次';

  @override
  String get verifyKitBackToKit => '回到装备箱';

  @override
  String get verifyKitSubmit => '校验';

  @override
  String get onboardingLocalNoticeTitle => '无云副本';

  @override
  String get onboardingLocalNoticeSubtitle => '此配置文件只存在于此设备上。请保留您的恢复工具并进行备份。';

  @override
  String get onboardingLocalNoticeKitTitle => '保护好你的救护箱';

  @override
  String get onboardingLocalNoticeKitBody => '如果你忘了密码,它打开了保险库';

  @override
  String get onboardingLocalNoticeBackupsTitle => '制作加密备份';

  @override
  String get onboardingLocalNoticeBackupsBody => '导出 a .ccbackup 文件, 或者让 ConsoleCrypt 自动返回到文件夹。';

  @override
  String get onboardingLocalNoticeLossTitle => '没有备份丢失的设备 = 丢失的数据';

  @override
  String get onboardingLocalNoticeLossBody => '没有服务器副本可以恢复。您可以在设置后期启用同步。';

  @override
  String get onboardingLocalNoticeSetUpBackups => '设置自动备份';

  @override
  String get onboardingLocalNoticeContinue => '我知道,继续';

  @override
  String unlockTitle(String name) {
    return '解锁$name';
  }

  @override
  String get unlockTitleGeneric => '解锁保险库';

  @override
  String get unlockPassphraseLabel => '保险库口令';

  @override
  String get unlockSubmit => '解锁';

  @override
  String get unlockOsAuthReason => '打开你的ConsoleCrypt保险库';

  @override
  String unlockWithOsAuth(String method) {
    return '与$method解锁';
  }

  @override
  String get unlockUntrustedTitle => '此设备尚未信任';

  @override
  String get unlockUntrustedMessage => '解锁您的口令, 或者从您已经使用的密码中批准此设备。批准需要比较两个屏幕上的校验代码。';

  @override
  String get unlockApproveFromOtherDevice => '从其他设备中批准';

  @override
  String get unlockForgotPassphrase => '忘了口令吗?';

  @override
  String get unlockSignOut => '退出登录';

  @override
  String get recoveryTitle => '恢复保险库访问';

  @override
  String get recoverySubtitle => '忘了你的保险库密码?';

  @override
  String get recoveryMethodRecoveryKey => '回收密钥';

  @override
  String get recoveryMethodTrustedDevice => '此可信任的设备';

  @override
  String get recoveryWordsInstructions => '键入您回收套件中的24个字, 或粘贴其文本QR 代码。';

  @override
  String get recoveryInputHint => '单词1 word2... word24 或控制台crypt- recovery:v1:...';

  @override
  String get recoveryScanQr => '扫描QR';

  @override
  String get recoveryQrFromImage => 'QR 从图片';

  @override
  String get recoveryQrNotFound => '找不到回收 KitQR。请尝试更清晰、更接近的图片或输入单词。';

  @override
  String get recoveryScanQrHelp => '将相机指向您的回收套件的QR代码。框架留在这个设备上。';

  @override
  String get recoveryScanQrUnavailable => '相机不可用。允许相机在设置中访问或使用 QR 图片。';

  @override
  String get recoveryQrRecognised => 'QR 文本得到承认';

  @override
  String get recoveryQrIncomplete => 'QR 文本不完整';

  @override
  String recoveryWordCount(int count, int total) {
    return '$count / $total 词';
  }

  @override
  String recoveryTrustedDeviceInfo(String method) {
    return '$method 释放此设备的解锁密钥打开保险库。然后设置一个新的口令; 其他设备继续工作。';
  }

  @override
  String get recoveryDeviceNotTrusted => '此设备对保险库不可信。请使用您的回收密钥。';

  @override
  String get recoveryNewPassphraseLabel => '新建保险库口令';

  @override
  String get recoveryRepeatNewPassphraseLabel => '重复新口令';

  @override
  String get recoverySubmitTrustedDevice => '认证和设定口令';

  @override
  String get recoverySubmitRecoveryKey => '恢复和设置口令';

  @override
  String get recoveryLostEverythingTitle => '口令、恢复密钥和每个可信任的设备丢失了吗 ?';

  @override
  String get recoveryLostEverythingLocal =>
      '本地配置文件只能从一个.ccbackup 的口令或恢复密钥( 欢迎 ~ 从备份中恢复) 打开的文件中返回。没有它, 数据就无法恢复 —— 从设计上看, 没有人持有主密钥。';

  @override
  String get recoveryLostEverythingSynced => '旧的保险库不能被任何人,包括您的服务器——通过设计来解密。您可以通过电子邮件恢复您的账户并启动一个新的空保险库。';

  @override
  String get shellCollapseSidebar => '折叠侧边栏到图标';

  @override
  String get shellExpandSidebar => '展开侧边栏';

  @override
  String get shellCollapseSidebarShort => '折叠';

  @override
  String shellActionWithShortcut(String action, String shortcut) {
    return '$action ( $shortcut )';
  }

  @override
  String get shellNewConnection => '新连接';

  @override
  String shellNewConnectionTooltip(String shortcut) {
    return '新连接 $shortcut.';
  }

  @override
  String get shellSyncNever => '永远';

  @override
  String get shellSyncDetails => '同步细节';

  @override
  String aboutTitle(String appName) {
    return '关于$appName';
  }

  @override
  String get aboutTagline => '端到端加密SSH 客户端';

  @override
  String aboutVersion(String version) {
    return '版本 $version (中文(简体) ).';
  }

  @override
  String get aboutAuthor => '作者';

  @override
  String get aboutLicenses => '许可证';

  @override
  String get aboutLicenseClient => '桌面客户端';

  @override
  String get aboutLicenseServer => '同步服务器';

  @override
  String get aboutOpenSource => 'ConsoleCrypt是开源软件。您的连接是安全的。';

  @override
  String get settingsAboutTitle => '关于';

  @override
  String get settingsAppearanceTitle => '外观';

  @override
  String get settingsSecurityTitle => '警卫';

  @override
  String get settingsGlassHelp => '如何通过侧边栏、工具栏、菜单和对话框。安全提示总是不透明的。';

  @override
  String get settingsGlassSolidReduceTransparency => '显示固态, 因为在系统设置中 降低透明度。';

  @override
  String get settingsGlassSolidRemote => '在远程桌面会话中显示固态。';

  @override
  String get settingsGlassSolidBattery => '显示固态以保存电池( 电池保存器 / 低功率模式) 。';

  @override
  String get settingsGlassSolidPerformance => '玻璃减少的性能。';

  @override
  String get settingsSidebarLabel => '边栏';

  @override
  String get settingsSidebarFloating => '浮动';

  @override
  String get settingsSidebarEdgeToEdge => '边缘到边缘';

  @override
  String get secretFieldCyrillicHint => '键盘文字包含西里尔字母——检查键盘布局.';

  @override
  String get secretFieldNonLatinHint => '键盘文字包含在 A–Z 外的字母——检查键盘布局.';

  @override
  String get passphraseLayoutNotice => '此口令包含 A– Z 以外的字母。要解锁保险库, 您必须使用相同的键盘布局键入它。';

  @override
  String get unlockDeleteProfile => '删除此配置文件...';

  @override
  String get deleteProfileLocalWarning => '此保险库仅保存在当前设备上。删除后，只能通过 .ccbackup 备份文件和对应的口令或恢复套件恢复。没有备份，保险库中的主机、密钥和密码将永久丢失。';

  @override
  String get deleteProfileAcknowledge => '我知道此操作无法撤销';

  @override
  String runFlowConfirmTitleDestructive(String host) {
    return '运行一个破坏性命令 在$host?';
  }

  @override
  String get paletteGroupActions => '行动';

  @override
  String get paletteGroupAi => '大赦国际';

  @override
  String get settingsUiFontScale => '界面文本大小';

  @override
  String get settingsUiScaleHelp => '100% 是新的基数大小, 相当于先前的90% 。选择从 80% 到 140%。终端文本大小是分开配置的。';

  @override
  String get settingsAccentColor => '角颜色';

  @override
  String get settingsBackgroundTint => '背景线';

  @override
  String get settingsColorsHelp => '选择 swatch 或输入任意 HEX 颜色。背景适应光或暗的主题; 文本可读。';

  @override
  String get settingsColorApply => '应用';

  @override
  String get settingsColorHex => 'HEX 颜色';

  @override
  String get settingsColorInvalid => '输入六个十六进制数字,例如#4C8DFF.';

  @override
  String get settingsResetUi => '重置文本大小和颜色';

  @override
  String get settingsTerminalColorScheme => '终端颜色主题';

  @override
  String get settingsTerminalThemeHelp => '立即应用到所有终端标签上,独立于界面颜色.';

  @override
  String get settingsTerminalThemeSystem => '跟随接口';

  @override
  String get settingsTerminalThemeDark => '石墨画';

  @override
  String get settingsTerminalThemeLight => '纸张';

  @override
  String get settingsTerminalThemeMidnight => '午夜时分';

  @override
  String get settingsTerminalThemeOcean => '海洋';

  @override
  String get settingsTerminalThemeForest => '森林';

  @override
  String get settingsTerminalThemeAmber => '琥珀';

  @override
  String get settingsTerminalPreview => '终端预览';

  @override
  String get settingsAppearanceLocalHint => '此设备已保存。不要求重新启动。';

  @override
  String get errorSecureStore =>
      '系统安全存储没有可用。在macOS 上, “ 登录” 键链请求您的 Mac 登录密码, 而不是您的保险库密码。如果您的 Mac 密码更改了, 键链仍然可以使用旧密码。打开 键链访问并解锁“ 登录 ”, 然后重试打开配置文件。不要删除或重置键链: 它持有此设备的加密密钥。';

  @override
  String get welcomeResumeTitle => '打开配置文件';

  @override
  String get welcomeResumeHelp => '您的配置文件在此设备上。请选择一个要继续。';

  @override
  String get welcomeKeychainHelp =>
      '打开配置文件可能会请求访问系统密钥链。在macOS 上, 输入“ 登录” 密钥链的密码( 通常是您的Mac 登录密码) 。如果被拒绝, 请在 Keychain Access 中解锁“ 登录 ”; 在Mac 密码更改后, 可能需要先前的密码。';

  @override
  String get reopenLastProfile => '启动时打开最后一个配置文件';

  @override
  String get reopenLastProfileHelp => '可以在应用程序打开前显示系统密钥链提示。';

  @override
  String get settingsTerminalThemeCustom => '自定义';

  @override
  String get colorSpectrum => '颜色谱';

  @override
  String get colorHue => '色调';

  @override
  String get colorSaturation => '饱和度';

  @override
  String get colorBrightness => '亮度';

  @override
  String get commonThemeColor => '颜色';

  @override
  String get terminalThemeEdit => '自定义/ 导入...';

  @override
  String get terminalThemeEditorHelp => '选择要编辑的颜色。当您按“应用”键时,修改适用于所有终端标签。';

  @override
  String get terminalThemeImport => '导入主题';

  @override
  String get terminalThemeExport => '导出JSON';

  @override
  String get terminalThemeFormats => '导入: iterm.ittermcolors(XML)或ConsoleCrypt.json,最多256 KB. 输出: ConsoleCrypt.json.';

  @override
  String get terminalThemeImportError => '无法导入此主题。选择有效的 iTermion XML 预设或 ConsoleCrypt JSON 文件, 最多为 256 KB。您保存的主题没有更改。';

  @override
  String get terminalThemeExportError => '无法保存主题。选择另一个位置并再次尝试。';

  @override
  String get terminalColorBackground => '背景情况';

  @override
  String get terminalColorForeground => '文本';

  @override
  String get terminalColorCursor => '光标';

  @override
  String get terminalColorSelection => '选择';

  @override
  String terminalColorAnsi(int index) {
    return 'ANSI$index号)';
  }

  @override
  String terminalColorAnsiBright(int index) {
    return '亮度$index';
  }

  @override
  String get terminalThemeFileType => '终端颜色方案';

  @override
  String get terminalThemeJsonType => 'ConsoleCrypt主题';

  @override
  String get terminalSampleSelection => ' 选中文本 ';

  @override
  String get settingsGlassRetryEffects => '重试玻璃效果';

  @override
  String get securityKeyTitle => '实物安保钥匙';

  @override
  String get securityKeyHelp => '用USB安全密钥(YubiKey 或其他FIDO2密钥)及其PIN解锁保险库.';

  @override
  String get securityKeyOnHelp => '打开 : 锁屏提供此密钥解锁。只对此设备上的这个保险库, 口令和恢复密钥仍然有效。';

  @override
  String get securityKeyTouch => '如果被询问,请输入密钥PIN,并在闪烁时触摸密钥.';

  @override
  String get securityKeyTouchTitle => '触摸安全钥匙';

  @override
  String get securityKeyTouchHelp => '键是眨眼,等待触摸:按下其按钮或触摸金接触.';

  @override
  String get securityKeyTouchTwice => '切换它需要两个触摸: 第二次在键再次眨眼之后.';

  @override
  String get securityKeyTouchTimeout => '未触摸请求在30秒后取消.';

  @override
  String get securityKeyEnabled => '安全密钥解锁开启';

  @override
  String get securityKeyPinTitle => '安全钥匙PIN';

  @override
  String get securityKeyPinHelp => '输入密钥的FIDO2(PIN,然后在闪烁时触摸密钥.';

  @override
  String get securityKeyPinLabel => 'PIN';

  @override
  String get securityKeyPinContinue => '继续';

  @override
  String get securityKeyErrorNoDevice => '没有找到安全密钥。插入密钥并再次尝试。';

  @override
  String get securityKeyErrorSeveral => '多个安全密钥已连接, 只留下一个。';

  @override
  String get securityKeyErrorPin => '安全密钥错误PIN.';

  @override
  String get securityKeyErrorPinBlocked => '安全密钥PIN 被屏蔽。解锁口令。';

  @override
  String get securityKeyErrorCancelled => '已取消或未及时触碰密钥。';

  @override
  String get securityKeyErrorWrongKey => '这不是为这个保险库设置的安全钥匙';

  @override
  String get securityKeyErrorUnsupportedKey => '此密钥不支持hmac- secret扩展FIDO2。';

  @override
  String get securityKeyErrorUnsupported => '此设备不支持安全密钥。';

  @override
  String get securityKeyErrorDevice => '安全密钥没有响应, 重新连接并再次尝试。';

  @override
  String get unlockWithSecurityKey => '打开安全密钥';

  @override
  String get deviceUnlockTitle => '指纹解锁';

  @override
  String get deviceUnlockHelp => '系统仅在您的指纹,面部或设备密码后释放保险库密钥.';

  @override
  String get deviceUnlockLocalOnly => '仅用于此设备上的保险库。您的口令和恢复密钥仍然可用。';

  @override
  String get deviceUnlockEnableReason => '允许在此设备上解锁您的ConsoleCrypt 保险库';

  @override
  String get deviceUnlockNeedsTrust => '首先打开保险库 用它的口令在这个设备上。';

  @override
  String get deviceUnlockNotEnrolled => '没有注册生物鉴别技术。在设备的系统设置中设置指纹或面部识别,然后再次检查可用性。';

  @override
  String get deviceUnlockUnsupported => '此设备目前没有生物计量解锁。请检查系统设置中的生物计量支持和设置。';

  @override
  String get deviceUnlockMacPassword => 'macOS目前提供它的账户密码. 触摸ID需要注册指纹:系统设置 → 触摸ID & 密码.';

  @override
  String get deviceUnlockRecoveryUnavailable => '解锁此设备已禁用或目前无法使用。使用您的恢复密钥。您可以在签名后启用 : 设置 ~ 默认。';

  @override
  String get deviceUnlockRefresh => '再次检查可用性';

  @override
  String deviceUnlockWithMethod(String method) {
    return '与$method解锁';
  }

  @override
  String get deviceUnlockMacPasswordName => 'macOS密码';

  @override
  String get errorOsAuthInvalidated => '设备上的指纹或登录方式已更改。请用保险库口令解锁，然后在“设置 → 保险库”中重新启用设备解锁。';

  @override
  String deviceUnlockProtectedBySystem(String method) {
    return '受系统保护:密钥只在$method之后发布.';
  }

  @override
  String get deviceUnlockConvenienceWarning => '在用户账户下运行的任何程序都可以解锁没有口令的解锁。这是方便而非保护。';

  @override
  String get deviceUnlockConvenienceConfirmTitle => '没有密码就解锁?';

  @override
  String get deviceUnlockConvenienceConfirmAction => '打开吧';

  @override
  String get deviceUnlockKeyringName => '系统密钥环';

  @override
  String deviceUnlockReofferMigratedTitle(String method) {
    return '转回$method?';
  }

  @override
  String get deviceUnlockReofferMigratedMessage => '与此设备解锁现在受系统保护。它在更新过程中被关闭; 用一个确认再次打开。';

  @override
  String get deviceUnlockReofferInvalidatedTitle => '指纹已更改';

  @override
  String deviceUnlockReofferInvalidatedMessage(String method) {
    return '$method 密钥已无效, 因为设备的登录更改了。打开此设备打开吗 ?';
  }

  @override
  String get deviceUnlockReofferEnable => '打开';

  @override
  String get deviceUnlockReofferLater => '现在不行';

  @override
  String get recoveryDeviceResetReason => '重置ConsoleCrypt 保险库口令';

  @override
  String get workspaceToolsClose => '关闭右面板';

  @override
  String get workspaceToolsResize => '右面板宽度';

  @override
  String get snippetPackage => '软件包';

  @override
  String get snippetPackageHint => '选择已有名称或输入新名称。没有软件包,请留空。';

  @override
  String get snippetAllPackages => '所有片断';

  @override
  String get snippetNoPackage => '没有包裹';

  @override
  String get snippetStarterCatalog => '启动软件包';

  @override
  String get snippetStarterIntro => '在您的保险库中添加有用的命令。您可以在此之后编辑或删除这些命令; 更改您的保险库同步。';

  @override
  String snippetStarterOrigin(String package) {
    return '从$package启动包.';
  }

  @override
  String snippetAddCount(int count) {
    return '添加$count命令';
  }

  @override
  String get snippetPackageAdded => '已添加';

  @override
  String get snippetPackageRename => '重命名软件包';

  @override
  String get snippetPackageDelete => '删除软件包';

  @override
  String snippetPackageDeleteMessage(int count) {
    return '删除此包中的全部$count 片段吗 ? 此删除也会同步到您的其他设备。';
  }

  @override
  String get snippetMovePackage => '移动到软件包...';

  @override
  String get snippetPaste => '粘贴';

  @override
  String get snippetRunMany => '在多个终端中运行...';

  @override
  String get snippetChooseTerminals => '选择终端';

  @override
  String get snippetConnectedOnly => '仅列出连接的终端。运行前将检查命令和所有目标。';

  @override
  String get snippetNoConnected => '先连接一个终端';

  @override
  String get snippetTargetsChanged => '配置文件或选定的终端已更改。再次选择目标。';

  @override
  String get snippetUnsafePaste => '此终端不支持安全的多行粘贴。在执行前使用 Run来审查脚本。';

  @override
  String get snippetSyncHint => '启用同步时保存在保险库中。';

  @override
  String get snippetPackLinux => 'Linux 诊断';

  @override
  String get snippetPackLinuxDescription => '系统负载,磁盘,内存,端口和服务日志. for Linux hosts.';

  @override
  String get snippetPackDocker => 'Docker';

  @override
  String get snippetPackDockerDescription => '集装箱、编组、资源使用和日志,对主机的要求Docker。';

  @override
  String get snippetPackKubernetes => 'Kubernetes';

  @override
  String get snippetPackKubernetesDescription => '节点、节点、服务、日志和事件。需要配置的 kubectl 上下文。';

  @override
  String get snippetCatalogUptime => '系统负载和升空时间';

  @override
  String get snippetCatalogDisk => '磁盘空间';

  @override
  String get snippetCatalogMemory => '内存使用情况';

  @override
  String get snippetCatalogPorts => '监听端口';

  @override
  String get snippetCatalogJournal => '服务日志';

  @override
  String get snippetCatalogContainers => '所有容器';

  @override
  String get snippetCatalogCompose => '构建项目状态';

  @override
  String get snippetCatalogStats => '集装箱资源使用情况';

  @override
  String get snippetCatalogDockerLogs => '集装箱记录';

  @override
  String get snippetCatalogDockerDisk => 'Docker磁盘使用';

  @override
  String get snippetCatalogNodes => '集群节点';

  @override
  String get snippetCatalogPods => '名片空间中的 pods';

  @override
  String get snippetCatalogServices => '命名空间服务';

  @override
  String get snippetCatalogPodLogs => 'Pod 日志';

  @override
  String get snippetCatalogEvents => '最近举行的分组活动';

  @override
  String get glassScrollTabsLeft => '向左滚动标签';

  @override
  String get glassScrollTabsRight => '向右滚动标签';

  @override
  String inventoryOverview(int hosts, int groups) {
    return '$hosts 主办单位$groups 组';
  }

  @override
  String get inventoryAllHosts => '所有主机';

  @override
  String get inventoryUngrouped => '未分组';

  @override
  String get inventoryFolders => '集团';

  @override
  String get inventoryLocation => '地点';

  @override
  String get inventoryExpand => '扩展组';

  @override
  String get inventoryCollapse => '折叠组';

  @override
  String get inventorySearch => '搜索主机、组、地址或标签';

  @override
  String get inventoryCards => '纸牌';

  @override
  String get inventoryList => '列表';

  @override
  String get inventoryGroupSettings => '组合设置';

  @override
  String get inventoryEffectiveDefaults => '保存的设置和继承';

  @override
  String get inventoryIncludesSubgroups => '本组内的主机及其分组';

  @override
  String get inventoryEmptyHelp => '添加主机或组来组织您的服务器。';

  @override
  String get inventorySearchHelp => '尝试另一次搜索或清除过滤器。';

  @override
  String get inventoryClearFilters => '清除过滤器';

  @override
  String get inventoryConnected => '活动SSH 连接';

  @override
  String get settingsWorkspacePanel => '右侧面板';

  @override
  String get settingsWorkspaceFloating => '烟泡';

  @override
  String get settingsWorkspaceExpanded => '统一面板';

  @override
  String get settingsWorkspaceFloatingHelp => '工具图标旁边分开浮动面板。此偏好保存在此设备上。';

  @override
  String get settingsWorkspaceExpandedHelp => '右侧的栏杆会用片段和 AI 聊天标签扩展成一个面板, 它在宽的窗口中共享工作空间, 在窄的窗口中覆盖它。保存在这个设备上。';

  @override
  String get workspaceToolsCollapse => '折叠右侧面板';

  @override
  String get mobileMore => '更多';

  @override
  String get mobileKeyboard => '键盘';

  @override
  String get mobileTools => '工具';

  @override
  String get mobileMessageHint => '信件...';

  @override
  String get mobileDeviceAuth => '指纹或PIN';

  @override
  String get mobileDeviceAuthHelp => '使用指纹传感器或手机屏幕锁确认身份。';

  @override
  String get mobileBackupsHelp => '手动加密备份可以保存到文件。 Android 上尚未提供自动预定的备份。';

  @override
  String get sftpDefaultEditor => '默认编辑器';

  @override
  String get sftpSystemEditor => '系统默认';

  @override
  String get sftpDefaultEditorHelp => '在编辑器中打开和打开此应用程序。打开时允许您为一个文件选择另一个应用程序。只保存在这个设备上。';

  @override
  String get terminalContextMenu => '终端动作';

  @override
  String get terminalPaste => '粘贴';

  @override
  String get terminalSaveSnippet => '另存为片段...';

  @override
  String get terminalSelectAll => '选择整个缓冲';

  @override
  String get terminalClearSelection => '清除选择';

  @override
  String get terminalAllowMultilinePaste => '允许粘贴多行文本而无需确认';

  @override
  String get terminalPasteConfirmTitle => '粘贴多行或控制字符 ?';

  @override
  String get terminalPasteConfirmMessage => '此文本可能执行命令或中断运行进程。在粘贴前审查它。';

  @override
  String aiTerminalAttachment(String host) {
    return '选中文本 ^ $host';
  }

  @override
  String get aiRemoveAttachment => '删除选中文本';

  @override
  String get screenCaptureTitle => '截图和屏幕记录';

  @override
  String get screenCaptureAllow => '允许截图';

  @override
  String get screenCaptureHelp => '还允许最近应用中的屏幕记录和应用程序预览。关闭以隐藏应用程序内容不被捕获。';

  @override
  String get screenCaptureLocalOnly => '立即应用到此设备上的整个应用程序, 包括锁屏。重新启动后保存; 从未同步。';

  @override
  String get screenCaptureMacosHelp => '屏幕截图是允许的。当前版本macOS 无法为此应用程序提供可靠的方式来阻断截图或屏幕记录。';

  @override
  String get screenCaptureError => '无法读取或应用截图设置。刷新状态并再次尝试。';

  @override
  String get updatesTitle => '最新情况';

  @override
  String get updateAutomatic => '启动时检查更新';

  @override
  String get updatePrivacyHelp => '只有应用版本和平台被使用,故障数据,密码和账户符号从未发送.';

  @override
  String get updateCheck => '检查更新';

  @override
  String updateCurrentVersion(String version) {
    return '已安装版本:$version';
  }

  @override
  String get updateChecking => '正在检查更新...';

  @override
  String get updateCurrent => '你有最新的版本。';

  @override
  String updateAvailable(String version) {
    return '现有版本$version';
  }

  @override
  String get updateDownloadInstall => '下载和安装';

  @override
  String updateDownloading(int percent) {
    return '下载:$percent%';
  }

  @override
  String get updateInstalling => '正在验证并启动安装器...';

  @override
  String get updateInstallHelp => '安装器将更新ConsoleCrypt, 并可能关闭应用程序及其会话SSH。您的配置文件和保险库将被保存。 Android 请您确认安装。';

  @override
  String get updateMacInstallHelp =>
      '下载后, 请选择在 macOS 保存对话框中保存已验证的 DMG 的位置。安装器将打开。退出 ConsoleCrypt, 将应用程序拖入应用程序并确认替换。您的配置文件和保险库被保存。';

  @override
  String get updateAndroidPermission => '允许ConsoleCrypt在打开的Android设置中安装更新,然后按下下载并再次安装.';

  @override
  String get updateInstallerOpened => '系统安装器打开, 完成安装。';

  @override
  String get updateFailed => '无法完成更新。';

  @override
  String get updateFailedHelp => '检查您的连接并再次尝试。无法签名或检查和校验的文件从未安装。';

  @override
  String get updateAppleHelp => '在iOS上,更新通过App Store,TestFlight或Xcode安装,这取决于应用程序的安装方式.';

  @override
  String get updateLinuxInstallHelp =>
      '软件包被下载并对照签名进行检查SHA-256,然后系统要求管理员密码和软件包管理器(apt,dnf或zypper)安装,配置文件,保险库和系统密钥环保存;新版本在重启后打开.';

  @override
  String get updateLinuxInstalled => '更新已安装。重新启动ConsoleCrypt 打开新版本。';

  @override
  String get updateRelaunch => '重新开始';

  @override
  String get updateLinuxNoPkexec => 'pkexec (polkit), 该应用程序用于请求管理员密码, 但未找到。用 apt 或 dnf 手动安装下载页面中的软件包。';

  @override
  String updateLinuxInstallFailed(String code) {
    return '软件包管理器以代码$code退出. 使用apt或dnf手动安装下载页面中的软件包.';
  }

  @override
  String get updateLinuxHelp => '在 Linux 上, 从官方网站下载您分发的最新 .deb 或 rpm 软件包, 并与您的软件包管理器一起安装。使用同样的方法更新应用程序。';

  @override
  String get updateLinuxDownloadPage => '官方下载页面';

  @override
  String get sharingTitle => '共享';

  @override
  String get sharingSubtitle => '此服务器上已验证设备的选定数据。';

  @override
  String get sharingUnavailable => '此服务器无法共享。';

  @override
  String get sharingLocal => '连接配置文件到服务器共享。';

  @override
  String get sharingReceived => '随时随地';

  @override
  String get sharingOwned => '与我同在 与我同在';

  @override
  String get sharingPublish => '共享...';

  @override
  String get sharingPublishConfirm => '份额';

  @override
  String get sharingEmpty => '尚未共享项目。';

  @override
  String get sharingReceivedHelp => '您的同事与您共享的项目。请在接受访问前检查所有者。';

  @override
  String get sharingOwnedHelp => '管理您共享的项目、收件人权限和访问取消。';

  @override
  String get sharingReceivedEmpty => '还没跟你分享呢';

  @override
  String get sharingReceivedEmptyHelp => '要求此服务器上的同事共享一个项目, 然后刷新此页面。在接受该设备前, 请先比较所有者的设备代码。';

  @override
  String get sharingOwnedEmpty => '你还没有分享任何东西';

  @override
  String get sharingOwnedEmptyHelp => '从项目菜单中选择共享。选择收件人并验证其设备代码。只有您选择的项目才会共享。';

  @override
  String get sharingDevicesTitle => '在另一个设备上访问';

  @override
  String get sharingDevicesHelp => '在现有共享访问中添加一个设备。设备验证和所有者的权限仍然适用。';

  @override
  String get sharingDevicePrepare => '添加此设备';

  @override
  String get sharingDeviceEndorse => '确认另一个设备';

  @override
  String get sharingDeviceContinue => '继续添加设备';

  @override
  String get sharingDeviceMore => '更多行动';

  @override
  String get sharingDeviceMoreHelp => '继续使用已被确认的设备包, 或验证已签名的历史以恢复本地请求状态。这些行动本身并不允许访问。';

  @override
  String get sharingInvitation => '邀请——核实所有者';

  @override
  String get sharingAccept => '校验和接受';

  @override
  String get sharingRefresh => '刷新共享数据';

  @override
  String get sharingIdentity => '我的设备密钥指纹';

  @override
  String get sharingVerifyHelp => '这是您设备的密钥指纹。要和同事检查密钥, 请在设备旁边使用“ 验证 ” : 通过语音比较安全号码或亲自扫描QR 代码。登录和电子邮件不会验证设备密钥。';

  @override
  String get sharingCode => '核查代码';

  @override
  String get sharingConfirmed => '我同事设备上的密码';

  @override
  String get sharingEmail => '同事的电子邮件';

  @override
  String get sharingFind => '查找设备';

  @override
  String get sharingSelectDevice => '选择和验证接收设备';

  @override
  String get sharingReader => '读取';

  @override
  String get sharingEditor => '编辑';

  @override
  String get sharingReaderHelp => '可读取并保存副本; 无法更改共享项目。';

  @override
  String get sharingEditorHelp => '可以读取和编辑内容;所有者管理访问.';

  @override
  String get sharingPreview => '分享什么';

  @override
  String get sharingExclude => '密码, SSH 密钥, 个人参考文献, 终端历史和 AI 密钥除外。请在共享前检查命令和自由文本。';

  @override
  String get sharingIncludeNotes => '包含主机笔记';

  @override
  String get sharingSecretWarning => '收件人可以读取并保守这个秘密。取消访问不能删除已经读取的副本。 SSH 访问还需要更改目标服务器上的证书。';

  @override
  String get sharingSecretConfirmed => '我理解并允许这个秘密 与选定的设备分享';

  @override
  String get sharingRdpHost => 'RDP主机';

  @override
  String get sharingRdpHelp => '该卡带有地址,端口,用户,域和标记. Notes和保存的密码仅在选中时共享.';

  @override
  String get sharingRdpIncludePassword => '包含已保存的密码';

  @override
  String get sharingRdpPasswordWarning => '收件人会得到密码并保存它。只能与您信任的已核实的人分享。';

  @override
  String get sharingRdpCopyPassword => '用我的副本保存共享密码';

  @override
  String get sharingRdpNoPassword => '密码不共享: 连接时会询问密码。';

  @override
  String get sharingHost => '主机';

  @override
  String get sharingSnippet => '片断';

  @override
  String get sharingGroup => '共享收藏';

  @override
  String get sharingSecret => '机密';

  @override
  String get sharingDevice => '设备';

  @override
  String get sharingOwner => '拥有者';

  @override
  String get sharingMembers => '成员';

  @override
  String get sharingVerified => '已验证';

  @override
  String get sharingBlocked => '需要审查';

  @override
  String get sharingDeleted => '删除';

  @override
  String get sharingRevision => '订正';

  @override
  String get sharingQueue => '发送队列';

  @override
  String get sharingFlush => '发送待定更改';

  @override
  String get sharingPending => '待决';

  @override
  String get sharingConflictHelp => '访问或内容更改。刷新项目并审查更改后再发送。';

  @override
  String get sharingRevoke => '取消访问';

  @override
  String get sharingRevokeHelp => '未来的更新将使用新的密钥。已经读取的副本将保留给收件人。';

  @override
  String get sharingStop => '删除共享项目';

  @override
  String get sharingStopHelp => '共享项目将删除,供与会者使用。';

  @override
  String get sharingCopyPersonal => '保存个人副本';

  @override
  String get sharingCopyHelp => '副本保存在您的个人保险库中。证书是分开配置的, 不与同事共享。';

  @override
  String get sharingSaved => '完成';

  @override
  String get sharingEdit => '编辑共享项目';

  @override
  String get sharingChangeSaved => '更改已保存或排队发送。';

  @override
  String get sharingName => '名称';

  @override
  String get sharingAddress => '地址';

  @override
  String get sharingPort => '端口';

  @override
  String get sharingUsername => '用户名';

  @override
  String get sharingNotes => '笔记';

  @override
  String get sharingDescription => '说明';

  @override
  String get sharingCommand => '命令';

  @override
  String get sharingNoDevices => '找不到设备。请检查地址和您同事的电子邮件验证。';

  @override
  String get sharingNoAutoRun => '收到的命令不会自动运行。';

  @override
  String get sharingOwnerDeviceOnly => '在创建此项目的所有者设备上管理访问。';

  @override
  String get sharingWorking => '正在准备安全共享...';

  @override
  String get sharingCopyCode => '复制指纹';

  @override
  String get sharingAddRecipient => '添加收件人';

  @override
  String get sharingRefreshCopy => '刷新个人副本';

  @override
  String get sharingManageAccess => '管理访问';

  @override
  String get sharingReviewQueue => '审查队列';

  @override
  String get sharingQueueHelp => '当访问或内容更改时, 更改会暂停。请审查当前项目; 在创建新更改前丢弃过时的请求。';

  @override
  String get sharingDiscard => '丢弃队列请求';

  @override
  String get sharingDiscardHelp => '从此设备中删除此请求? 服务器已经接受的更改仍然保留。丢弃请求不会取消已公布的访问。';

  @override
  String get sharingReconcile => '校验历史';

  @override
  String get sharingReconcileHelp => '本地副本位于受保护的检查点后方。所有者的密钥必须是这个设备上的密钥; app将验证完整的签名历史, 并拒绝缺失或相互冲突的修改。';

  @override
  String get sharingEndpointChanged => '共享主机地址或端口已更改。在连接到您的证书之前, 检查并确认新的端点。';

  @override
  String get sharingTunnelVerify => '在手动启动隧道前验证共享主机的访问权限。';

  @override
  String get sharingPromptPassword => '连接时询问密码';

  @override
  String get sharingRefreshHost => '刷新共享主机';

  @override
  String get sharingRefreshHostHelp => '在共享中审查地址和端口。将更新的终点应用到您的个人副本中? 您的其他连接设置被保存。';

  @override
  String get sharingDetachHost => '拆卸个人副本';

  @override
  String get sharingDetachHostHelp => '此副本将停止检查更改和访问共享主机。 Collegues 保留自己的访问和副本。';

  @override
  String get sharingCollectionHelp => '选择独立共享的项目。一组只包含引用, 不允许访问其内容。';

  @override
  String get sharingCollectionEmpty => '分别分享所需项目.';

  @override
  String get sharingCollectionUnavailable => '无法获取的项目: 需要单独访问。';

  @override
  String get sharingCollectionChildren => '分组项目';

  @override
  String get sharingSecretSelection => '要分享的秘密';

  @override
  String get sharingSecretPrimary => '密码或私钥';

  @override
  String get sharingSecretPassphrase => '密钥口令( 单独的项目)';

  @override
  String get enrollmentTitle => '新个人设备';

  @override
  String get enrollmentHelp => '许可适用于一个共享项目。一个同事信任的设备证实了他们的新设备;所有者留在网上以授予访问权限。个人保险库数据从未转移。';

  @override
  String get enrollmentCreate => '允许新设备';

  @override
  String get enrollmentAnchor => '信任的确认设备';

  @override
  String get enrollmentExpiry => '许可寿命';

  @override
  String get enrollmentQuota => '新设备数量';

  @override
  String get enrollmentOneDay => '1天 时间';

  @override
  String get enrollmentSevenDays => '7天时间';

  @override
  String get enrollmentThirtyDays => '30天时间';

  @override
  String get enrollmentAutomatic => '自动接受确认的设备';

  @override
  String get enrollmentAutomaticHelp => '仅在信任设备的代码比较之后, 而所有者在线。新设备不能超过选中的权限。';

  @override
  String get enrollmentDisable => '禁用权限';

  @override
  String get enrollmentDisableHelp => '此权限将停止接受新设备。在管理访问中分别删除先前允许的设备。即使服务器无法访问, 禁用仍然记录在这个设备上。';

  @override
  String get enrollmentExport => '新设备软件包';

  @override
  String get enrollmentRequests => '设备请求';

  @override
  String get enrollmentPrepare => '我在新设备上';

  @override
  String get enrollmentEndorse => '确认另一个设备';

  @override
  String get enrollmentSubmit => '继续添加设备';

  @override
  String get enrollmentPackage => '设备包';

  @override
  String get enrollmentPackageHelp => '在您的设备中传输此公开软件包。它包含密钥和签名确认, 没有密码或共享的项目内容。代码比较仍然需要。';

  @override
  String get enrollmentPreview => '审查包';

  @override
  String get enrollmentCodeHelp => '两个设备都显示此请求的相同代码。从另一个设备的屏幕中键入代码以确认您比较它。';

  @override
  String get enrollmentCopyPackage => '复制软件包';

  @override
  String get enrollmentSavePackage => '将软件包保存到文件';

  @override
  String get enrollmentOpenPackage => '从文件打开软件包';

  @override
  String get enrollmentPending => '我的设备要求';

  @override
  String get enrollmentWait => '请求已发出。请将此设备和所有者的设备上网。当所有者发出核查挑战时继续。';

  @override
  String get enrollmentChallenge => '校验设备密钥';

  @override
  String get enrollmentRespond => '继续核查';

  @override
  String get enrollmentAccept => '赠款准入';

  @override
  String get enrollmentManualConfirmed => '我同意用显示的权限添加这个设备';

  @override
  String get enrollmentActive => '活动';

  @override
  String get enrollmentDisabled => '已禁用';

  @override
  String get enrollmentRequested => '等待核查';

  @override
  String get enrollmentChallenged => '等待设备响应';

  @override
  String get enrollmentResponded => '设备密钥已校验';

  @override
  String get enrollmentAccepted => '准许访问';

  @override
  String get enrollmentExpired => '过期';

  @override
  String get enrollmentBlocked => '必须审查目前的准入和历史';

  @override
  String get enrollmentCreateConfirmed => '我授权在选定的限度内添加这个用户自己的设备';

  @override
  String glassOpenTerminalTabs(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count 开放终端', one: '1个开放终端');
    return '$_temp0';
  }

  @override
  String get updateMacInstallerOpened => 'DMG已经保存并打开. Quit ConsoleCrypt,在应用程序中替换应用程序,然后再次推出.';

  @override
  String get updateMacSaveTitle => '保存ConsoleCrypt 更新';

  @override
  String get updateMacSavePrompt => '保存和打开';

  @override
  String get updateMacDestinationExists => '此文件已经存在。重试安装, 并为新的 DMG 选择其它名称或文件夹。';

  @override
  String get rdpTitle => '远程桌面';

  @override
  String get rdpNewConnection => '新建RDP 连接';

  @override
  String get rdpEmptyTitle => '标签中的桌面';

  @override
  String get rdpEmptyHelp => '输入 aWindows 服务器和账户。在签名前验证服务器证书。';

  @override
  String get rdpLocked => '打开您的保险库打开 RDP。';

  @override
  String get rdpConnecting => '正在连接...';

  @override
  String get rdpConnected => '已连接';

  @override
  String get rdpDisconnected => '连接已关闭。创建连接以再次签名。';

  @override
  String get rdpConnectionFailed => '连接失败。请检查地址、账户和证书。';

  @override
  String get rdpSecureAttention => 'Ctrl + Alt + 删除';

  @override
  String get rdpResize => '屏幕 1920×1080';

  @override
  String get rdpExpand => '全屏幕';

  @override
  String get rdpCollapse => '退出全屏幕';

  @override
  String get rdpAddress => '服务器地址';

  @override
  String get rdpPort => '端口';

  @override
  String get rdpUsername => '用户名';

  @override
  String get rdpDomain => '域( 可选)';

  @override
  String get rdpPassword => 'Windows密码';

  @override
  String get rdpCredentialsHelp => '密码只用于此签名, 而不保存。在确认证书之前不会发送。';

  @override
  String get rdpInvalidForm => '输入地址,一个从1到65535的端口,用户名和密码.';

  @override
  String get rdpCertificateTitle => '验证RDP证书';

  @override
  String get rdpCertificateHelp => '在信任的频道上将整个指纹与您的管理员进行比较。确认仅适用于此连接。如果不符, 取消。';

  @override
  String get rdpFingerprint => '证书SHA-256';

  @override
  String get rdpCertificateConfirm => '我比较了整个证书指纹';

  @override
  String get rdpCertificateChanged => '服务器证书已更改。签名被屏蔽; 创建新连接并验证指纹。';

  @override
  String get rdpAuthenticationFailed => 'Windows 拒绝登录。请检查用户名、域名和密码。';

  @override
  String get rdpResizeUnavailable => '服务器不支持此屏幕大小。继续原大小。';

  @override
  String get rdpWorkspaceHelp => '标签中的远程桌面。剪贴板和选定的文件夹只有在您允许的情况下才能使用。';

  @override
  String get rdpPermissions => '会话权限';

  @override
  String get rdpAllowClipboard => '允许文本剪贴板';

  @override
  String get rdpClipboardHelp => 'Ctrl+V(在Mac上的QQV) 发送和粘贴文本。单独发送和接收交换文本。默认关闭访问。';

  @override
  String get rdpFolderHelp => '只让远程计算机访问到选定的文件夹。默认情况下只读。';

  @override
  String get rdpSelectFolder => '选择文件夹...';

  @override
  String get rdpStopFolder => '断开文件夹';

  @override
  String get rdpNoFolder => '未共享文件夹';

  @override
  String get rdpAllowFolderWrite => '允许写入此文件夹';

  @override
  String get rdpFolderWriteHelp => '远程计算机将能够在此文件夹中创建、更改和删除文件。';

  @override
  String get rdpFolderUnsupported => '此设备无法共享文件夹。';

  @override
  String get rdpPermissionsFailed => '无法应用权限。请再试一次。';

  @override
  String get rdpClipboardReceived => '收到文本。剪贴板将根据您的安全设置过期。';

  @override
  String get rdpClipboardSent => '文本发送到远程计算机.';

  @override
  String get rdpClipboardFailed => '无法交换剪贴板文本。';

  @override
  String get rdpSendClipboard => '发送剪贴板文本';

  @override
  String get rdpReceiveClipboard => '接收剪贴板文本';

  @override
  String get rdpSavedPasswordHelp => '密码将取自此主机的加密证书。';

  @override
  String get hostProtocolLabel => '连接类型';

  @override
  String get hostRdpConnectionHelp => 'RDP直接连接. 端口和证书被设定在这个主机上; SSH 组设置不适用.';

  @override
  String get hostRdpPasswordOnly => '选择 RDP 的密码证书。';

  @override
  String get hostRdpUsernameRequired => '输入Windows 用户名';

  @override
  String get rdpFolderPending => 'Windows正在连接文件夹...';

  @override
  String get rdpFolderReady => '文件夹连接。在Windows 文件浏览器中打开。';

  @override
  String get rdpFolderDenied => 'Windows 拒绝文件夹。请您的管理员允许驱动重定向; 远程桌面仍然连接。';

  @override
  String get rdpFolderUnavailable => 'Windows 没有连接文件夹。请检查服务器是否允许驱动器重定向, 然后重新连接。';

  @override
  String get rdpFolderChooseFailed => '无法打开选中的文件夹。请检查访问权限并再次选择。';

  @override
  String get rdpFolderChooseLimit => '无法选择其他文件夹。取消未使用的选择或关闭不需要的会话, 再试一次。';

  @override
  String get rdpFolderOpenHelp => '连接后, 在Windows 中打开此 PC 或输入地址栏中的此路径 :';

  @override
  String get rdpFolderWindowsPath => '\\ ts客户端\\ ConsoleCrypt';

  @override
  String get rdpFolderLimits => '每个会话中最多有256个 MiB 和 512个 miB 写。链接和特殊文件没有可用。更改文件夹会中断当前打开的文件。';

  @override
  String get rdpClipboardUnavailable => '文本剪贴板尚未准备好或被服务器禁用。等待并再次尝试; 如果错误持续存在, 请与您的管理员检查重定向设置。';

  @override
  String get rdpClipboardLimit => '只有文本最多可达64 KiB才能被传输.';

  @override
  String get rdpClipboardBusy => '上一个剪贴板请求仍在处理中。等待并再次尝试。';

  @override
  String get rdpClipboardEmpty => '剪贴板中没有可用的文本。';

  @override
  String get rdpPinBar => 'Pin 连接栏';

  @override
  String get rdpUnpinBar => '自动隐藏连接栏';

  @override
  String get rdpConnections => '切换连接';

  @override
  String get rdpMinimize => '最小化窗口';

  @override
  String get rdpDisconnect => '断开此会话';

  @override
  String get rdpShowBar => '显示连接栏( Ctrl+ Alt+ Home)';

  @override
  String get rdpFullscreenFailed => '无法更改全屏模式。请尝试窗口控制。';

  @override
  String get rdpMobileTouchpad => '触摸盘';

  @override
  String get rdpMobileDirectTouch => '直接触摸';

  @override
  String get rdpMobileShowKeyboard => '显示键盘';

  @override
  String get rdpMobileHideKeyboard => '隐藏键盘';

  @override
  String get rdpMobileLeftClick => '左键点击光标';

  @override
  String get rdpMobileRightClick => '光标右键点击';

  @override
  String get rdpMobileHelpTitle => '电话控制';

  @override
  String get rdpMobileHelp =>
      '桌面是一个触摸板。滑动一个手指来移动光标; 抬起并重新定位您的手指来继续。\n\n点击到左键, 点击两次到双键。按住第二个按键并移动到拖动。您也可以按住一个手指半秒, 然后拖动。\n\n 两根手指:按住右键,一起滑动滚动,按住缩放,从 1× 缩放到 4× 。双键有两个手指来配合桌面。放大后,图片会跟随光标。\n\n 用三根手指按住键盘以显示或隐藏键盘。它会覆盖桌面而不会缩小它。在密钥中, 将 Ctrl, Alt, Win 或 Shift 一次用于下一个密钥, 两次锁定并再次释放。\n\n圆形按钮打开菜单。将其拖到边缘。切换到直接触摸、更改解析度、切换会话或在此最小化。附加的鼠标会保持其正常控制。';

  @override
  String get localAiTitle => '当地人工智能';

  @override
  String get localAiDescription => '选择此计算机的模型。安装后, 聊天和命令建议会离线。';

  @override
  String get localAiModel => '型号';

  @override
  String localAiDownloadSize(String size) {
    return '型号下载:$size';
  }

  @override
  String get localAiNotInstalled => '未安装';

  @override
  String get localAiInstalling => '正在下载和校验文件...';

  @override
  String get localAiStopped => '已安装 ^ 当您发出请求时开始';

  @override
  String get localAiStarting => '开始模型...';

  @override
  String get localAiReady => '准备... 运行于此计算机';

  @override
  String get localAiStopping => '正在停止中...';

  @override
  String get localAiError => '当地人工智能需要注意';

  @override
  String get localAiUnsupported => '可用支持的计算机Windows、macOS和Linux。';

  @override
  String get localAiDefault => '安装后默认使用';

  @override
  String get localAiDeviceOnly => '仅在此计算机上。其他设备和保存的提供者设置保持独立。';

  @override
  String get localAiInstall => '安装本地 AI';

  @override
  String get localAiStart => '开始';

  @override
  String get localAiStop => '停下来';

  @override
  String get localAiRemove => '删除选中的模式';

  @override
  String get localAiResourceHelp => '在运行时使用内存和CPU; 在15分钟后不请求停止。小模型可能会出错, 在运行前审查命令。';

  @override
  String get localAiOperationFailed => '本地的 AI 操作无法完成。请检查它的状况并再次尝试。';

  @override
  String get localAiInstallFailed => '安装失败。请检查互联网连接和空闲的磁盘空间, 然后再试一次。';

  @override
  String get localAiFilesChanged => '无法校验已安装的文件。删除模型并再次安装。';

  @override
  String get localAiStartFailed => '该模型无法意外启动或停止。释放一些内存并再次尝试。';

  @override
  String get localAiStorageUnavailable => '无法访问本地的 AI 文件。请检查 app 数据文件夹的权限和磁盘空格。';

  @override
  String localAiRamRecommendation(int ram) {
    return '建议的总RAM: ${ram}GB. 速度取决于您的硬件.';
  }

  @override
  String get localAiModelsPreserved => '切换模型保存其他下载。安装模型和您的选择在应用程序更新后幸存。';

  @override
  String get localAiTierCustom => '自定义';

  @override
  String get localAiImport => '添加自己的模型( GGUF)...';

  @override
  String get localAiImportHelp =>
      '该文件被复制到ConsoleCrypt模型文件夹,并由SHA-256检查;您可以删除原件。任何内置的 lama.cpp 读取的 GGUF 都会这样做;内存指导是从文件大小中估计出来的.';

  @override
  String get localAiCustomModelNotice => '你自己的模特儿:检查它的起源,并给自己发牌.';

  @override
  String get localAiInvalidGguf => '这不是 GGUF 模型文件。';

  @override
  String get localAiImportFailed => '无法添加模型。请检查空格。';

  @override
  String get localAiCustomMissing => '缺少自定义模型文件。删除后再添加。';

  @override
  String get localAiModelInstalled => '已安装';

  @override
  String get localAiRuntimeMissing => '推论引擎缺失。修复它; 下载的模型将被保存。';

  @override
  String get localAiBundledRuntimeMissing => '该应用程序的推论引擎无法使用。程序将保存下载的模型。';

  @override
  String get localAiRepairRuntime => '修理引擎';

  @override
  String get localAiInstallSelectedHelp => '安装选中的模型以使用本地 AI。您的本地默认保存已启用; 请求不会切换到云提供者。';

  @override
  String get localAiTierLight => '浅色';

  @override
  String get localAiTierMedium => '中型';

  @override
  String get localAiTierStrong => '重型';

  @override
  String localAiModelRequirements(String size, int ram) {
    return '$size下载_${ram}GB RAM推荐.';
  }

  @override
  String get localAiRecommended => '建议该计算机的总内存';

  @override
  String localAiDetectedMemory(String size) {
    return '这台计算机: $size 总RAM。恒星是一个建议; 您的选择保持不变。';
  }

  @override
  String get localAiMetalHelp => '苹果硅使用金属加速度,较大的模型需要更多的内存,并且可以更慢的反应.';

  @override
  String get localAiCpuHelp => 'Windows, Linuxx64和Intel Mac使用CPU. RAM 制导不能保证快速响应;更大的模型可以缓慢.';

  @override
  String localAiModelAttribution(String publisher, String license) {
    return '型号/GGUF:$publisher 许可证:$license';
  }

  @override
  String get localAiModelNotices => '示范来源和许可证';

  @override
  String get terminalLocalSearch => '搜索贝壳或SSH主机';

  @override
  String get terminalLocalSection => '本地弹壳';

  @override
  String get terminalLocalSshSection => 'SSH 主机';

  @override
  String get terminalLocalComputer => '这台电脑';

  @override
  String get terminalLocalUnavailable => '没有本地贝壳可用.';

  @override
  String get terminalLocalSandboxUnavailable => '这个沙箱建筑里没有本地贝壳。';

  @override
  String get terminalLocalRefresh => '刷新本地贝壳';

  @override
  String get terminalLocalEmptyMessage => '在新标签中打开本地 shell 或连接到 SSH 主机。';

  @override
  String get terminalLocalOpen => '打开终端';

  @override
  String get terminalLocalRestart => '重新启动 shell';

  @override
  String get terminalLocalInsertOnly => '插入命令,审查命令,然后按 Enter 本地终端。';

  @override
  String get rdpMobileMenu => '会话控制';

  @override
  String get rdpMobileKeys => '键';

  @override
  String get rdpMobileFit => '适应桌面';

  @override
  String get rdpMobileFitResolution => '匹配屏幕分辨率';

  @override
  String get rdpMobileOrientation => '方向';

  @override
  String get rdpMobileAutomatic => '自动';

  @override
  String get rdpMobileLandscape => '景观';

  @override
  String get rdpMobilePortrait => '纵向';

  @override
  String get rdpMobileMinimize => '最小化会话';

  @override
  String get rdpMobileBackAgain => '按回键以最小化会话';

  @override
  String get rdpMobileDisconnectConfirm => '从远程桌面断开连接吗 ?';

  @override
  String get rdpMobileClipboard => '剪贴板';

  @override
  String get rdpMobileIntroMove => '将手指滑到桌面上移动光标。抬起并重新定位手指继续。';

  @override
  String get rdpMobileIntroZoom => '两根手指:按右键,滑动滚动,捏缩缩.';

  @override
  String get rdpMobileIntroMenu => '键盘用三个手指按住。浮动的圆键打开菜单; 拖到任意边。';

  @override
  String get rdpMobileDontShow => '不再显示';

  @override
  String get rdpMobileSettingsTitle => 'RDP 电话控制';

  @override
  String get rdpMobileSettingsLocal => '这些首选项会保留在这个设备上。最后一个输入模式会被单独记住每个主机。';

  @override
  String get rdpMobileDefaultMode => '默认输入模式';

  @override
  String get rdpMobileAutoDefault => '电话上的触摸板,直接触摸平板电脑';

  @override
  String get rdpMobileSpeed => '指针速度';

  @override
  String get rdpMobileAcceleration => '指向加速';

  @override
  String get rdpMobileCursorSize => '光标大小';

  @override
  String get rdpMobileAutoResize => '将分辨率匹配到屏幕';

  @override
  String get rdpMobileClarity => '清晰度';

  @override
  String get rdpMobileWindowsScale => '比额表Windows';

  @override
  String get rdpMobileAutoHide => '自动隐藏控制';

  @override
  String get paletteNewLocalTerminal => '新建本地终端';

  @override
  String get hostImportTitle => '导入系统主机';

  @override
  String get hostImportHelp => '从已知的_hosts 中选择地址。此文件没有用户名或密码; 导入后设置它们。信任的密钥不会更改; 在第一次连接时验证服务器密钥。';

  @override
  String get hostImportReadFailed => '无法读取已知的_hosts 文件。请检查访问权限和文件大小( 最多 4 MiB) 。';

  @override
  String get hostImportSaveFailed => '无法保存所有主机。已保存的主机被标记在列表中; 重试以添加剩下的主机。';

  @override
  String hostImportSkipped(int count) {
    return '跳过条目:$count. Hashed 地址,模式,CA记录,撤销和错误的密钥不会创建主机.';
  }

  @override
  String hostImportSaved(int count) {
    return '主机增加:$count.';
  }

  @override
  String get hostImportSelectAll => '选择所有新主机';

  @override
  String get hostImportEmpty => '没有找到要导入的地址。文件可能只包含散列名称, 无法找到。';

  @override
  String get hostImportExists => '已经在此简介中';

  @override
  String get commonImport => '导入';

  @override
  String get kubeTitle => 'Kubernetes';

  @override
  String get kubeSubtitle => '您加密保险库中的集群连接。';

  @override
  String get kubeImport => '导入 kubeconfig';

  @override
  String get kubeImportHelp => '选择 kubeconfig 或 粘贴 YAML。选择要保存的上下文。导入时不会连接到集群或运行认证命令。';

  @override
  String get kubeChooseFile => '选择文件';

  @override
  String get kubeYaml => '库贝库维奇YAML';

  @override
  String get kubePreview => '审查背景';

  @override
  String get kubeSaveContexts => '保存选中上下文';

  @override
  String get kubeChooseAgain => '选择另一个配置';

  @override
  String get kubeEmpty => '还没有 Kubernetes 连接。导入 kubeconfig 以添加上下文。';

  @override
  String get kubeNamespace => '命名空间';

  @override
  String get kubeAuthentication => '认证';

  @override
  String get kubeProduction => '生产:防止变化';

  @override
  String get kubeApproveFiles => '为选定的上下文读取这些引用文件';

  @override
  String get kubeExecHelp => '此上下文使用外部认证命令。导入它不会允许运行该命令。';

  @override
  String get kubeImportInvalid => '此上下文无法导入。请检查源配置中的组、用户和认证设置。';

  @override
  String get kubeImportFailed => '无法导入配置。请检查选中的上下文和文件, 然后再次审查。';

  @override
  String get kubeFileFailed => '选择一个不大于 4 MiB 的正则文件 UTF-8。';

  @override
  String get kubeExpired => '此预览已过期。再次审查配置。';

  @override
  String get kubeTlsRequired => '生产需要验证TLS 证书。校正 kubeconfig 或将此标为非生产上下文。';

  @override
  String get kubeRoute => '连接路径';

  @override
  String get kubeRouteDirect => '直接连接';

  @override
  String get kubeRouteRequired => '此 kubeconfig 指定了代理。请选择一条可以到达API 服务器的保存的SSH 路由。';

  @override
  String get kubeSaved => '已保存的选定上下文。';

  @override
  String get kubeDeleteHelp => '从您的保险库中删除此已保存的上下文吗 ? 这不会改变 Kubernetes 组中的任何内容。';

  @override
  String get kubeLoadFailed => '无法装入Kubernetes 连接。';

  @override
  String get kubeRefresh => '刷新连接';

  @override
  String get kubeContextLimit => '每次导入最多选择64个上下文。';

  @override
  String get kubeAuthToken => '托肯语';

  @override
  String get kubeAuthCertificate => '客户端证书';

  @override
  String get kubeAuthExec => '外部命令';

  @override
  String get kubeAuthNone => '未配置';

  @override
  String get kubeConnect => '连接';

  @override
  String get kubeDisconnect => '断开连接';

  @override
  String get kubeConnecting => '正在连接到集群...';

  @override
  String get kubeConnectionFailed => '无法连接。请检查网络、认证和连接路径。';

  @override
  String get kubeRetry => '再试一次';

  @override
  String get kubeBackToContexts => '已保存的连接';

  @override
  String get kubeReadOnly => '生产——只读';

  @override
  String get kubeReadOnlyHelp => '这种上下文由本地客户端保护Kubernetes不会发生变化.';

  @override
  String get kubeResources => '资源';

  @override
  String get kubeAllNamespaces => '所有命名空间';

  @override
  String get kubeResourceKind => '资源类型';

  @override
  String get kubeLabelSelector => '标签选择器';

  @override
  String get kubeApplyFilter => '应用过滤器';

  @override
  String get kubeNoResources => '没有资源匹配这些过滤器。';

  @override
  String get kubeNoResourceKinds => '没有发现可读资源类型。请检查此账户的权限Kubernetes。';

  @override
  String get kubeRequestFailed => '无法装入集群数据。请检查连接和Kubernetes 权限。';

  @override
  String get kubeNamespacesFailed => '命名空间列表没有。您仍然可以在选中的名称空间查看资源。';

  @override
  String get kubeLoadMore => '装入更多';

  @override
  String get kubeRefreshResources => '刷新资源';

  @override
  String get kubeLive => '实时更新';

  @override
  String get kubeSnapshot => '抓图';

  @override
  String get kubeWatchFailed => '直播更新停止。刷新以重新连接。';

  @override
  String get kubeResourceLimit => '内存限制为10,000资源,缩小命名空间或标签过滤器。';

  @override
  String get kubeOverview => '概览';

  @override
  String get kubeObjectYaml => 'YAML';

  @override
  String get kubeLogs => '日志';

  @override
  String get kubeContainer => '集装箱';

  @override
  String get kubePreviousLogs => '上一个容器';

  @override
  String get kubeLogFilter => '过滤日志行';

  @override
  String get kubeNoLogs => '无日志行可显示。';

  @override
  String get kubeLogTailHelp => '最后500行被装入内存。刷新以获取新尾巴。';

  @override
  String get kubeSecretRedacted => '隐藏秘密值。此视图只包含元数据和密钥名称。';

  @override
  String get kubeStatus => '状态';

  @override
  String get kubeCreatedAt => '已创建';

  @override
  String get kubeLabels => '标签';

  @override
  String get kubeConditions => '条件';

  @override
  String get kubeOwners => '拥有者';

  @override
  String get kubeName => '名称';

  @override
  String get kubeNotReported => '未报告';

  @override
  String get kubeSessionEnded => '连接已经结束。重新连接到加载集群数据。';

  @override
  String kubeLoadedCount(int count) {
    return '$count 资源已装入';
  }

  @override
  String get kubeShowDetails => '打开资源细节';

  @override
  String kubeClusterVersion(String version) {
    return 'Kubernetes $version';
  }

  @override
  String get kubeSortName => '按名称排序';

  @override
  String get kubeSortNamespace => '按命名空间排序';

  @override
  String get kubeSortStatus => '按状态排序';

  @override
  String get kubeSortAge => '最新的第一个';

  @override
  String get kubeRefreshDetails => '刷新细节';

  @override
  String get kubeAllowChanges => '允许更改十分钟';

  @override
  String get kubeAllowChangesHelp => '这将打开此生产背景的临时写入窗口。更改将被记录在本地审计日志中。请输入保险库口令以继续。';

  @override
  String get kubeCloseWriteWindow => '仅返回读取';

  @override
  String kubeWriteWindowRemaining(String time) {
    return 'PROD 允许更改 ——$time 剩余';
  }

  @override
  String get kubeWriteWindowFailed => '无法更改写入窗口。重新连接并再次尝试。';

  @override
  String get kubeEditYaml => '编辑 YAML';

  @override
  String get kubeApplyYaml => '应用更改';

  @override
  String get kubeApplyYamlHelp => '将已审查的修改应用到此资源中吗 ? 输入保险库口令以确认。如果资源版本已更改, 服务器将拒绝更新。';

  @override
  String get kubeApplyFailed => '更改未被接受。在再次尝试前刷新对象并检查权限、校验错误或所有权冲突。';

  @override
  String get kubeReviewChanges => '审查变动';

  @override
  String get kubeDryRun => '在服务器上验证';

  @override
  String get kubeDryRunRequired => '先验证服务器上的YAML, 在应用前审查所产生的更改。';

  @override
  String get kubeDryRunHelp => '服务器干运行: 删除的行被标记 − 和添加的行 +。目前还没有保存到集群。';

  @override
  String get kubeForceApply => '拥有冲突领域';

  @override
  String get kubeForceApplyHelp => '只有在您打算替换另一个控制器管理的字段时才启用。启用后再次验证更改。';

  @override
  String get kubeNoYamlChanges => '服务器返回编号 YAML 更改。';

  @override
  String get kubeDeleteResource => '删除Kubernetes资源';

  @override
  String get kubeDeleteResourceHelp => '这将从集群中删除资源, 并可能停止运行工作量。请输入保险库口令以确认。';

  @override
  String get kubeFollowLogs => '遵循新日志行';

  @override
  String get kubePauseLogs => '暂停';

  @override
  String get kubeResumeLogs => '继续';

  @override
  String get kubeLogBufferHelp => '直播日志缓冲器:最多2万行和2米B,仅保存在记忆中.';

  @override
  String get kubeLogOptions => '日志选项';

  @override
  String get kubeGitOps => '图形';

  @override
  String get kubeProblems => '问题';

  @override
  String get kubeHelm => '赫尔姆';

  @override
  String get kubeInsightsPartial => '由于权限、发现失败或大小限制,有些资源无法检查,结果不完整。';

  @override
  String get kubeGitOpsAll => '全部';

  @override
  String get kubeGitOpsReady => '准备';

  @override
  String get kubeGitOpsAttention => '需要关注';

  @override
  String get kubeGitOpsSuspended => '暂停';

  @override
  String get kubeDependencies => '依赖关系';

  @override
  String get kubeNoVisibleProblems => '在可以检查的资源中没有发现问题。';

  @override
  String get kubeNoProblems => '在检查的资源中没有发现已知的问题。';

  @override
  String get kubeNoGitOpsResources => '没有Flux或Argo CD资源匹配此过滤器.';

  @override
  String get kubeGitOpsActions => '主计长的行动';

  @override
  String get kubeDependencyCycle => '检测到依赖周期';

  @override
  String get kubeDependencyMissing => '未发现依赖性';

  @override
  String get kubeDependencyReady => '取决于可用的资源';

  @override
  String get kubeDependencyWaiting => '等待抚养';

  @override
  String get kubeGitOpsSuspend => '暂停同步';

  @override
  String get kubeGitOpsResume => '继续同步';

  @override
  String get kubeGitOpsReset => '重置失败计数器';

  @override
  String get kubeGitOpsForce => '强制直升机升级';

  @override
  String get kubeGitOpsReconcile => '现在同步';

  @override
  String get kubeArgoSync => '同步应用程序';

  @override
  String get kubeArgoRefresh => '刷新应用程序';

  @override
  String get kubeArgoHardRefresh => '刷新应用程序和清单';

  @override
  String get kubeGitOpsChangeHelp => '这将向控制器发送更改, 并会影响运行中的应用程序。审查动作并输入保险库口令以继续。';

  @override
  String get kubeGitOpsReason => '理由(储存在资源说明中)';

  @override
  String get kubeIssueCrashLoop => '集装箱不断坠毁';

  @override
  String get kubeIssueImagePull => '无法下载容器图像';

  @override
  String get kubeIssueContainerConfig => '容器配置无效';

  @override
  String get kubeIssueOom => '集装箱超过内存限制';

  @override
  String get kubeIssueRestarts => '集装箱多次重启';

  @override
  String get kubeIssueScheduling => '无法预定 Pod';

  @override
  String get kubeIssuePodNotReady => '波德还没准备好';

  @override
  String get kubeIssueNodeNotReady => '节点还没有准备好';

  @override
  String get kubeIssueNodePressure => '节点受到资源压力';

  @override
  String get kubeIssueCondition => '资源需要注意';

  @override
  String get kubeIssueRolloutDeadline => '工作量的推出超过了最后期限';

  @override
  String get kubeIssueReplicaFailure => '控制器无法创建复制件';

  @override
  String get kubeIssueReplicasUnavailable => '无法提供一些工作量复制件';

  @override
  String get kubeIssueVolumePending => '数量索赔正在等待储存';

  @override
  String get kubeIssueVolumeLost => '数量索赔损失';

  @override
  String get kubeIssueJobFailed => '任务失败';

  @override
  String get kubeIssueArgoDegraded => 'Argo CD 应用已退化';

  @override
  String get kubeIssueArgoOutOfSync => 'Argo CD 应用程序不同步';

  @override
  String get kubeIssueFluxPending => 'Flux 未观察到最新配置';

  @override
  String get kubeIssueFluxNotReady => '豪华资源尚未准备好';

  @override
  String get kubeIssueFluxStalled => 'Flux 同步因错误而停滞';

  @override
  String get kubeIssueFluxSuspended => '流同步暂停';

  @override
  String get kubeHelmAllRevisions => '显示所有修改';

  @override
  String get kubeHelmEmpty => '没有Helm释放匹配此过滤器。';

  @override
  String kubeHelmRevision(int revision) {
    return '订正$revision';
  }

  @override
  String get kubeHelmHistory => '发布历史';

  @override
  String get kubeHelmSecretsMasked => '敏感值被掩盖。值和表仅保留在记忆中。';

  @override
  String get kubeHelmValues => '用户值';

  @override
  String get kubeHelmChartValues => '图表默认';

  @override
  String get kubeHelmManifest => '声明';

  @override
  String get kubeHelmCompare => '与修订版相比';

  @override
  String get kubeHelmChooseRevision => '选择修订版';

  @override
  String get kubeHelmDiffHelp => '删除的行被标记 − 和添加的行 + 。此比较不会改变发布。';

  @override
  String get kubeExecReviewTitle => '允许认证命令吗 ?';

  @override
  String get kubeExecReviewHelp => '此命令在此计算机上运行。请检查它的可执行性、参数和环境。批准仅适用于此解锁配置文件; 更改的命令需要再次批准。';

  @override
  String get kubeExecArguments => '参数';

  @override
  String get kubeExecEnvironment => '环境';

  @override
  String get kubeExecAllow => '允许此命令';

  @override
  String get kubeAuthFailed => '认证无法完成。请检查连接设置并再次尝试。';

  @override
  String get kubeAuthExpired => '此请求已过期。重新开始认证。';

  @override
  String get kubeOidcTitle => '签名到集群';

  @override
  String get kubeOidcHelp => '在系统浏览器中与您的身份提供者签名。您的证书会停留在这个设备与提供者之间。';

  @override
  String get kubeOidcOpen => '打开浏览器';

  @override
  String get kubeOidcWaiting => '正在等待浏览器登录...';

  @override
  String get kubeAuthAction => '认证';

  @override
  String get kubeExecMobile => '外部认证命令无法在移动中访问。使用一个令牌、客户端证书或浏览器登录( OIDC) 。';

  @override
  String get kubePodTerminal => 'Pod 终端';

  @override
  String get kubeExecProductionBlocked => '终端接入因此生产背景而禁用.';

  @override
  String get kubeAttach => '附加到进程';

  @override
  String get kubeAttachHelp => '连接到容器的现有进程。键盘输入直接进入该进程。';

  @override
  String get kubeShell => '装在集装箱内的壳体';

  @override
  String get kubeOpenShell => '打开 shell';

  @override
  String get kubeTerminalFailed => '无法打开终端。请检查容器状态、权限和选中的 shell。';

  @override
  String get kubeHidePanel => '隐藏终端面板';

  @override
  String get kubeShowPanel => '打开终端面板';

  @override
  String get kubeTerminalEmpty => '从 Pod 的细节打开终端。';

  @override
  String get kubeTerminalConnected => '已连接';

  @override
  String get kubeTerminalDisconnected => '已断开连接';

  @override
  String get kubePortForward => '转发 Pod 端口';

  @override
  String get kubeForwardHelp => '该端口仅可在此设备上使用,时间为127.0.0.1. 当您断开集群或锁定配置文件时, 前端停止。';

  @override
  String get kubeForwardProductionBlocked => '用于此生产背景的端口转发已禁用。';

  @override
  String get kubeRemotePort => '集装箱TCP港)';

  @override
  String get kubeLocalPort => '本地端口';

  @override
  String get kubeLocalPortHelp => '使用 0 自动选择自由端口。';

  @override
  String get kubeStartForward => '开始转发';

  @override
  String get kubeForwardCreated => '正在运行中。在 Tunks 或集群面板中管理它。';

  @override
  String get kubeForwardFailed => '无法转发此端口。请检查 Pod 状态、权限和本地端口可用性。';

  @override
  String get kubePortForwards => 'Kubernetes 港口前进';

  @override
  String get kubeNoForwards => '无活动Kubernetes端口前进.';

  @override
  String get kubeForwardRunning => '运行';

  @override
  String get kubeForwardStopped => '已停止';

  @override
  String get kubeStopForward => '停止转发';

  @override
  String kubeForwardTraffic(int count, String sent, String received) {
    return '$count 发送$sent 收到$received';
  }

  @override
  String get kubePermissions => '访问权限';

  @override
  String get kubePermissionsHelp => '由Kubernetes归还的关于目前名称空间身份的规则仍然有效。';

  @override
  String get kubePermissionsIncomplete => '服务器返回了不完整的权限报告。缺少规则并不能证明禁止行动。';

  @override
  String get kubeScale => '规模';

  @override
  String get kubeRestart => '重新启动';

  @override
  String get kubeCordon => 'Cordon 节点';

  @override
  String get kubeUncordon => '解冻节点';

  @override
  String get kubeDrain => '排水节点';

  @override
  String get kubeDebugContainer => '添加调试容器';

  @override
  String get kubeMutationConfirmHelp => '审查操作并输入保险库密码以确认。 Kubernetes 执行前再次检查许可和生产保障。';

  @override
  String get kubeOperationFailed => '操作无法完成。请检查权限和资源状态, 然后在再次尝试前刷新。';

  @override
  String get kubeReplicas => '复制量(0-1 000 000)';

  @override
  String get kubeContainerImage => '容器图像';

  @override
  String get kubeDebugHelp => '此图像以 /bin/sh 开始于此 Pod。之后无法从 Pod 中移除麻黄容器。使用您信任的图像。';

  @override
  String get kubeDrainHelp => '节点已经被封锁。在驱逐前检查每个 Pod。守护进程和镜像 Pods 被跳过。中断预算仍然执行; 未管理 Pods 和本地空 Dir 数据屏蔽此操作。';

  @override
  String kubeDrainProgress(int done, int total) {
    return '已删除:$done和$total';
  }

  @override
  String get kubeDrainEmpty => '没有任何Pod人有资格被驱逐。';

  @override
  String get kubeDrainUnmanaged => '屏蔽:Pod没有控制器.';

  @override
  String get kubeDrainLocalData => '屏蔽:Pod使用本地空Dir数据.';

  @override
  String get kubeDrainBlocked => '屏蔽:在排水前审查这个Pod.';

  @override
  String get kubeMetrics => '资源使用情况';

  @override
  String get kubeMeasuredAt => '测量';

  @override
  String get kubeMetricsUnavailable => '无法使用量表。请检查测量服务器和访问权限。';

  @override
  String get kubeWorkloadPods => '工作量';

  @override
  String get kubeEvents => 'Kubernetes事件';

  @override
  String get kubeFluxCliTitle => '流量CLI';

  @override
  String get kubeFluxCliHelp => '可选桌面工具. 集群访问使用临时本地代理.';

  @override
  String get kubeFluxCliPath => '流量路径';

  @override
  String get kubeFluxCliFind => '查找';

  @override
  String get kubeFluxCliProbe => '检查版本';

  @override
  String get kubeFluxCliVersion => '发行版本';

  @override
  String get kubeFluxCliMethod => '安装方法';

  @override
  String get kubeFluxCliInstall => '安装';

  @override
  String get kubeFluxCliUpdate => '更新';

  @override
  String get kubeFluxCliRemove => '卸载';

  @override
  String get kubeFluxCliOfficial => '正式签名发布';

  @override
  String get kubeFluxCliConfirm => '确认 CLI 操作';

  @override
  String get kubeFluxCliVerify => '正式下载需要SHA-256和Cosign验证. 包命令在可见终端打开.';

  @override
  String get kubeFluxCliFailed => 'CLI行动失败或无法进行核查。';

  @override
  String get kubeFluxCliOutput => 'CLI 输出';

  @override
  String get kubeFluxCliRun => '运行';

  @override
  String get kubeFluxCliTerminal => '打开豪华终端';

  @override
  String get kubeFluxCliLocalPath => 'diff 的本地仓库目录';

  @override
  String get kubeFluxCliResource => '资源名称';

  @override
  String get kubeFluxCliKind => '资源种类';

  @override
  String get kubeNewConnection => '新连接';

  @override
  String get kubeConnectionSettings => '连接设置';

  @override
  String get kubeServerUrl => 'KubernetesAPI URL HTTPS ).';

  @override
  String get kubeCertificateAuthority => 'CA 证书( PEM, 可选)';

  @override
  String get kubeClientCertificate => '客户端证书( PEM)';

  @override
  String get kubePrivateKey => '客户端私钥( PEM)';

  @override
  String get kubeManualReviewHelp => '继续审查并导入此上下文。默认启用生产保护。';

  @override
  String get kubeProductionContext => '生产背景';

  @override
  String get kubeAllowExecInProd => '允许 Pod 终端生产';

  @override
  String get kubeAllowForwardInProd => '允许在生产中转发端口';

  @override
  String get kubeRecordEvents => '保存事件历史';

  @override
  String get kubeRecordEventsHelp => '在此设备上加密了7天。事件历史不同步。';

  @override
  String get kubeScanFolder => '扫描一个 kubeconfig 文件夹';

  @override
  String get kubeScanEmpty => '在此文件夹中找不到 kubeconfig 文件。';

  @override
  String kubeQueuedFiles(int count) {
    return '$count 排队审查的文件';
  }

  @override
  String get kubeNextFile => '查看下一个文件';

  @override
  String get kubeExport => '导出 kubeconfig';

  @override
  String get kubeExportHelp => 'kubeconfig 文件可以包含证书。只保存到信任的位置。';

  @override
  String get kubeIncludeCredentials => '包括全权证书';

  @override
  String get kubeExportSaved => 'Kubeconfig 保存了下来。';

  @override
  String get kubeAiAssistant => '大赦国际助理';

  @override
  String get kubeAiHelp => '只读分组工具。密件值被排除; Kubernetes 权限和您选择的上下文限制访问。答案永远不会运行命令。';

  @override
  String get kubeAiProvider => 'AI 供应商';

  @override
  String get kubeAiDefaultProvider => '此设备上的默认提供者';

  @override
  String get kubeAiConfigMaps => '允许读取这个问题的配置图值';

  @override
  String get kubeAiUnresolved => '一些检查无法完成;在得出结论之前审查工具错误。';

  @override
  String get kubeAiReadTool => '只读工具';

  @override
  String get kubeAiTruncated => '结果被截断。';

  @override
  String get kubeAiToolFailed => '此检查无法完成。数据可能缺失或访问可能受到限制。';

  @override
  String get kubeAiFailed => '助理无法完成请求。请检查选中的 AI 提供者和集群连接。';

  @override
  String get kubeAiQuestion => '询问此分组';

  @override
  String get kubeAiAsk => '问助理';

  @override
  String get kubeAiExternalTitle => '将集群上下文发送给此提供者吗 ?';

  @override
  String get kubeAiExternalHelp => '这个问题可能会向上述外部提供者发送已消毒的资源元数据、状态和日志。保密值仍然被排除在外。批准仅适用于这个问题。';

  @override
  String get kubeAiNoTools => '此提供者不能调用工具; 助手将只使用可用的只读上下文。';

  @override
  String get kubeAiExplain => '与大赦国际解释';

  @override
  String kubeAiExplainQuestion(String problem, String resource) {
    return '解释问题$problem 为$resource 。请在不改变资源的情况下检查相关状态和事件。';
  }

  @override
  String get sharingKubeContext => 'Kubernetes 语境';

  @override
  String get sharingKubeCluster => '分组';

  @override
  String get sharingKubeExclude => '默认情况下, 仅共享连接设置。认证、本地路由和批准。';

  @override
  String get sharingKubeIncludeCredentials => '包含认证证书';

  @override
  String get sharingKubeCredentialWarning => '收件人可以使用这些证书访问集群。仅与您信任的已核实的设备共享。撤销该共享不会撤销已被收件人复制的证书。';

  @override
  String get sharingKubeCredentialsIncluded => '包含认证; 值保持隐藏。';

  @override
  String get sharingKubeCredentialsExcluded => '认证不包括在内。';

  @override
  String get sharingKubeCopyHelp => '保存此连接的个人副本。选择您自己的路由。认证只通过您的明确选择导入; 可执行插件仍然需要此设备的批准。';

  @override
  String get sharingKubeImportCredentials => '导入包含的认证证书';

  @override
  String get sharingKubeRouteRequired => '此连接需要一条路由。在此设备上选择一个保存的SSH 主机或跳转配置。';

  @override
  String get sharingKubeUnsupported => '此服务器不支持共享Kubernetes 上下文。';

  @override
  String get sharingKubeSnapshotChanged => '如果此连接的设置或证书已改变, 请再次审查。';

  @override
  String get kubeHistory => '事件历史';

  @override
  String get kubeHistoryHelp => '过去7天的本地加密历史。启用连接设置中的记录。';

  @override
  String get kubeHistoryRecording => '记录集群事件';

  @override
  String get kubeHistoryDisabled => '历史记录被禁用';

  @override
  String get kubeHistoryConnecting => '正在重新连接事件记录器...';

  @override
  String get kubeHistoryUnavailable => '事件记录不可用;保存的历史仍可读。';

  @override
  String get kubeHistoryWarnings => '仅显示警告';

  @override
  String get kubeHistoryClear => '清除本地事件历史';

  @override
  String get kubeHistoryClearHelp => '删除此设备上下文中保存的事件历史。集群资源未受影响。';

  @override
  String get kubeHelmComputedValues => '计算值';

  @override
  String get kubeRevealSecret => '公开秘密值';

  @override
  String get kubeRevealSecretHelp => '值仅在此保护对话框中可见 30 秒。副本在 30 秒内从剪贴板上清除。值从未发送到 AI 或存储在资源历史中。';

  @override
  String get kubeSecretExpired => '价值隐藏,必要时再次曝光。';

  @override
  String get kubeCreateResource => '创建资源';

  @override
  String get kubeManifestTemplate => '插入元数据模板';

  @override
  String get kubeSchema => 'Kubernetes资源计划';

  @override
  String get kubeHelmRollback => '滚回Helm释放';

  @override
  String get kubeHelmRollbackHelp => '审查精确的 Helm 二进制、校验和命令。操作会改变集群资源; Helm 钩被禁用。请输入保险库密码以确认。';

  @override
  String get kubeHelmRollbackFailed => '回滚无法完成。请检查 Helm 路径、权限和当前发布版本。在重试前再次检查命令。';

  @override
  String get kubeHelmBinary => 'Helm 的绝对路径( 可选)';

  @override
  String get kubeAiHistory => 'AI 对话历史';

  @override
  String get kubeAiHistoryLocal => '在此设备上保存';

  @override
  String get kubeAiHistorySynced => '启用加密同步';

  @override
  String get kubeAiHistoryDeleteHelp => '删除此已保存的对话。如果同步, 其删除也会同步。';

  @override
  String get kubeRolloutHistory => '推出历史';

  @override
  String get kubeDiffBefore => '在此之前';

  @override
  String get kubeDiffAfter => '之后';

  @override
  String get kubeCompare => '比较';

  @override
  String get kubeDiffTruncated => 'diff 已截断。在应用更改前审查全部资源。';

  @override
  String get kubeNoChanges => '无变化';

  @override
  String get kubeDiffReview => '审查更改的字段。秘密值仍然隐藏。';

  @override
  String get kubeFluxCliAdviceMissing => '找不到奢侈CLI。安装它吗 ?';

  @override
  String get kubeFluxCliAdviceUpdate => '本集群的CLI比Flux年长。更新它吗 ?';

  @override
  String get kubeFluxCliNeverSuggest => '别再提了';

  @override
  String get kubeFluxCliLater => '现在不行';

  @override
  String get kubeFluxCliPathHelp => '添加流到路径';

  @override
  String get kubeFluxCliPathHint => '更新 PATH 后打开新的终端窗口。在 macOS / Linux 上, 将显示的行添加到指定的配置文件。';

  @override
  String get kubeFluxCliPathAdded => '该目录被添加到用户 PATH 中.';

  @override
  String get kubeFluxCliController => '集群中的流版';

  @override
  String get kubeGitOpsFlow => '资源流动';

  @override
  String get kubeGraphList => '列表';

  @override
  String get kubeGraphReset => '重置缩放和位置';

  @override
  String get kubeIssueCertificateExpiring => '证书即将过期';

  @override
  String get kubeIssueCertificateExpired => '证书已过期';

  @override
  String get kubeIssueEvicted => '波德被赶出来了';

  @override
  String get kubeIssuePendingResources => '分组资源不足';

  @override
  String get kubeIssuePendingTaints => '节点提示防止排程';

  @override
  String get kubeIssuePendingVolume => '波德在等一卷书';

  @override
  String get kubeIssueSuspendedLong => '同步暂停超过24小时';

  @override
  String get kubeIssueSourceStale => '来源文物已过时';

  @override
  String get kubeIssueHelmDrift => '赫尔姆被释放了';

  @override
  String get kubeIssueDependencyNotReady => '依赖还没有准备好';

  @override
  String get kubeIssueDependencyMissing => '缺少或没有依赖性';

  @override
  String get kubeExitCode => '退出代码';

  @override
  String get kubeCommandOpen => '打开Kubernetes 上下文';

  @override
  String get kubeCommandAsk => '问大赦国际Kubernetes';

  @override
  String get kubeCommandContext => '集群背景';

  @override
  String get kubeRelatedObjects => '相关资源';

  @override
  String get kubeRelationOwner => '拥有者';

  @override
  String get kubeRelationDependent => '依赖资源';

  @override
  String get kubeRelationVolume => '音量连接';

  @override
  String get kubeRelationConfiguration => '配置参考';

  @override
  String get kubeMetricsTrend => '本届会议期间CPU趋势';

  @override
  String get kubeLogSince => '时间范围';

  @override
  String get kubeLogAllTime => '永远';

  @override
  String get kubeLogLastMinute => '最后一分钟';

  @override
  String get kubeLogLastHour => '最后一个钟头';

  @override
  String get kubeLogLastDay => '最后一个';

  @override
  String get kubeLogRegex => '正则表达式';

  @override
  String get kubeLogRegexInvalid => '无效或过于复杂的模式。';

  @override
  String get kubeLogLevel => '职等';

  @override
  String get kubeLogAllPods => '所有舱位';

  @override
  String get kubeLogAllLevels => '各级';

  @override
  String get kubeLogWrap => '环行';

  @override
  String get kubeLogExport => '导出/ 共享日志';

  @override
  String get kubeLogExportHelp => '日志可能包含秘密。只有在当前过滤器下可见的行会被保存。';

  @override
  String get kubeLogMask => '导出前已识别机密';

  @override
  String get kubeLogTargetsTruncated => '显示前8个吊舱,打开一个特定的吊舱来查看其他人.';

  @override
  String get kubeLogSaved => '日志已保存。';

  @override
  String get kubeTags => '标记, 用逗号分隔';

  @override
  String get kubeTagsHelp => '集群标签和颜色由其保存的上下文共享.';

  @override
  String get kubeColor => '集群颜色';

  @override
  String get kubeNoColor => '无颜色';

  @override
  String get kubeFavorite => '切换最爱';

  @override
  String get kubeFavorites => '爱好';

  @override
  String get kubeAllKinds => '各类';

  @override
  String get kubeGroupWorkloads => '工作量';

  @override
  String get kubeGroupNetwork => '网络';

  @override
  String get kubeGroupConfig => '配置';

  @override
  String get kubeGroupStorage => '存储';

  @override
  String get kubeGroupAccess => '访问';

  @override
  String get kubeGroupCluster => '组群/CDRs';

  @override
  String get kubeLogDefaultContainer => '默认容器';

  @override
  String get kubeForwardChoosePod => '选择要转发的列表';

  @override
  String get kubeForwardChoosePodHelp => '选择目标 Pod。连接在确认其端口后才开始。';

  @override
  String get kubeMemoryTrend => '本会期内的记忆趋势';

  @override
  String get kubeRequests => '请求';

  @override
  String get kubeLimits => '限制';

  @override
  String get kubeCapacity => '能力';

  @override
  String get kubeAllocatable => '可填表';

  @override
  String get kubeResourceBudgetsUnavailable => '没有资源要求和限制。';

  @override
  String get kubeBulkActions => '选定资源';

  @override
  String get kubeBulkHelp => '检查准确的列表并输入保险库口令。操作按序运行; 取消在当前操作后停止。最多100 个资源。';

  @override
  String get kubeBulkProductionBlocked => '繁体变化在生产背景下被禁用.';

  @override
  String get kubeBulkFailed => '操作停止。已完成的资源已标记; 在重新试运行前审查剩余资源。';

  @override
  String get kubeContextTerminal => '此集群的本地终端';

  @override
  String get kubeContextTerminalHelp => '航站楼使用临时本地API代用. Cluster certain records stay inConsoleCrypt;生产限制仍然活跃.';

  @override
  String get kubeOutput => '输出';

  @override
  String get kubePinLogs => '在底板中打开日志';

  @override
  String get kubeResizePanel => '调整底板大小';

  @override
  String get kubeAiProposalReview => '审查建议';

  @override
  String get kubeAiProposalContext => '命令将不执行而插入到新的本地终端上下文。';

  @override
  String get kubeAiHistoryContext => '请求中的历史信息';

  @override
  String get kubeAiNewConversation => '新对话';

  @override
  String get kubeTableColumns => '列数';

  @override
  String get kubeAge => '年龄';

  @override
  String get kubeGitOpsSource => '来源';

  @override
  String get kubeGitOpsArtifact => '艺术';

  @override
  String get kubeGitOpsInterval => '调节间隔';

  @override
  String get kubeGitOpsRevision => '订正';

  @override
  String get kubeGitOpsSignature => '签字核查';

  @override
  String get kubeGitOpsInventory => '管理资源';

  @override
  String get kubeGitOpsValuesFrom => '参考值';

  @override
  String get kubeGitOpsControllerLogs => '控制器日志';

  @override
  String get kubeGitOpsPath => '仓库路径';

  @override
  String get kubeGitOpsBranch => '处';

  @override
  String get kubeGitOpsTag => '标记/ 版本限制';

  @override
  String get kubeGitOpsChart => '图表';

  @override
  String get kubeGitOpsDrift => '漂流探测';

  @override
  String get kubeGitOpsHistoryEmpty => '没有和解历史。';

  @override
  String get kubeGitOpsControllerUnavailable => '没有控制器日志。请检查资源发现和权限。';

  @override
  String get kubeGitOpsSignatureUnknown => '控制器未报告';

  @override
  String get kubeGitOpsSourceReconcileHelp => '请求此源的新调节。请输入保险库口令以继续。';

  @override
  String get kubeGitOpsMetadataOnly => '这里只显示对象引用。隐藏秘密值。';

  @override
  String get kubeResourceConfiguration => '工作负荷和节点配置';

  @override
  String get kubePort => '端口';

  @override
  String get kubePorts => '港口';

  @override
  String get kubeNode => '节点';

  @override
  String get kubeServiceAccount => '服务账户';

  @override
  String get kubeNodeSelector => '节点选择器';

  @override
  String get kubeReady => '准备';

  @override
  String get kubeRestarts => '重新启动';

  @override
  String get kubeIssueExitCode => '退出代码';

  @override
  String get kubeVolumeMounts => '音量挂载';

  @override
  String get kubeVolumes => '卷数';

  @override
  String get kubeUnschedulable => '已禁用时间安排';

  @override
  String get kubeTaints => '纹章';

  @override
  String get kubeNodeInfo => '节点软件';

  @override
  String get kubeGraphProblems => '解决问题的途径';

  @override
  String get kubeGraphSearch => '在图中查找资源';

  @override
  String get kubeGraphFold => '隐藏盘点叶';

  @override
  String get kubeReplicaStatus => '准备/想要的复制品';

  @override
  String get kubeTableSort => '排序资源';

  @override
  String get kubeForwardRestore => '重新连接后恢复';

  @override
  String get kubeForwardRestoreHelp => '仅当上下文保持打开且此配置文件解锁时。离开上下文即可澄清此选择。';

  @override
  String get kubeForwardRestoreFailed => '无法恢复一些端口前方。请检查吊舱和当地端口, 然后再次启动。';

  @override
  String get kubeGitOpsReconcileWithSource => '与源代码同步';

  @override
  String get kubeGitOpsReconcileWithSourceHelp => '同步源链, 然后同步此资源。控制器准备状态由运行状态显示。确认确切的资源名称。';

  @override
  String get kubeForwardNetwork => '允许从本地网络访问';

  @override
  String get kubeForwardBindAddress => '要监听的本地IP地址';

  @override
  String get kubeForwardNetworkWarning => '我理解其他设备可能不用Kubernetes认证就能进入这个端口。允许这个网络曝光。';

  @override
  String get kubeHelmComputedPartial => '从保存的发布中合并了数值;子图默认和全球进口可能缺失.';

  @override
  String get kubeManifestSnippets => '插入已保存的显示';

  @override
  String get kubeManifestSnippetsHelp => '选择一个包含 YAML 或 JSON 的 kubectl 片段。在服务器验证前审查编辑器中的标识符和变量。';

  @override
  String get kubeNoManifestSnippets => '尚未显示片段。请在片段中保存YAML 或JSON 作为 kubectl 片段。';

  @override
  String get kubeGitOpsSources => '资料来源';

  @override
  String get kubeGitOpsNotifications => '通知';

  @override
  String get kubeGitOpsImages => '图像自动化';

  @override
  String get kubeGitOpsRecent => '最近的控制器状态变化';

  @override
  String get kubeDockRight => '右侧嵌入面板';

  @override
  String get kubeDockBottom => '底部嵌入面板';

  @override
  String get kubeMaskedEditHelp => '此表单包含遮盖值。 YAML 更改被禁用, 以防止用占位符替换证书。使用单个资源动作或编辑原始信任的表单。';

  @override
  String get kubeAutoShell => '自动检测 shell';

  @override
  String get kubeGitOpsLocalAlerts => '此设备上的提醒和提醒';

  @override
  String get kubeGitOpsNotify => '通知豪华失败';

  @override
  String get kubeGitOpsNotifyHelp => '对于此上下文, 当应用程序打开时。 New Not ready 或 Stall 过渡以祝酒词和徽章的形式出现。';

  @override
  String kubeGitOpsFailureNotice(String resource, int count) {
    return '需要注意的流量:$resource$count';
  }

  @override
  String kubeGitOpsReminderDue(String resource, int count) {
    return '集群提醒:$resource$count';
  }

  @override
  String kubeGitOpsUnread(int count) {
    return '未读提醒 : $count';
  }

  @override
  String get kubeGitOpsCancelReminder => '取消提醒';

  @override
  String get kubeGitOpsRemind => '本地提醒';

  @override
  String get kubeGitOpsNoReminder => '不要提醒';

  @override
  String kubeGitOpsRemindHours(int hours) {
    return '在$hours中提醒 h';
  }

  @override
  String get kubePreferencesFailed => '无法保存或装入设备首选项。请再试一次。';

  @override
  String get kubeContextWslHelp => 'WSL需要Bash和WSL)1或WSL)2镜像网络. 主机回回路访问在发射前检查;NAT网络不支持.';

  @override
  String get kubeContextWslUnavailable => 'WSL无法连接到本地代理. use WSL 1, WSL 2 镜像网络或 PowerShell.';

  @override
  String get kubeKubectlMissing => '未找到 kubectl。它只用于本地集群终端。';

  @override
  String get kubeKubectlInstall => '安装 kubectl';

  @override
  String get kubeKubectlPackageHelp =>
      '软件包管理器配置决定了软件包的来源,版本和安装目录。将 kubectl 保存在一个小版本的集群中。命令运行在一个可见的终端中; 没有 kubeconfig 传递给它。';

  @override
  String get kubeKubectlDocs => '安装指令';

  @override
  String get kubeKubectlCheck => '检查 kubectl';

  @override
  String get kubeSchemaFields => '组合图中的字段';

  @override
  String get kubeSchemaHelp => '浏览字段及其来自集群 OpenAPI 的描述。插入一个字段只更改此编辑器; 在应用前验证该运算表。';

  @override
  String get kubeSchemaUnavailable => '集群计划没有可用。应用前仍然需要服务器验证。';

  @override
  String get kubeSchemaRequired => '要求数';

  @override
  String get kubeSchemaInsert => '在光标处插入字段';

  @override
  String get kubeSchemaNoFields => '没有匹配的策略。';

  @override
  String get kubeSchemaParent => '父字段';

  @override
  String get kubeSchemaRoot => '根字段';

  @override
  String get kubeGitOpsAppliedRevision => '应用修订';

  @override
  String get kubeGitOpsLatestRevision => '最近来源订正';

  @override
  String get kubeGraphExpand => '展开对象';

  @override
  String get kubeClusters => '集群';

  @override
  String get kubeExecInspect => '检查命令';

  @override
  String get kubeExecImportHelp => '导入前检查命令。导入从不运行它; 连接时需要单独确认执行。';

  @override
  String get kubeExecBinaryUnavailable => '此设备无法执行文件。执行前将检查其路径和SHA-256。';

  @override
  String get kubeGitOpsTimeline => '调节时间表';

  @override
  String get kubeHelmCompareLive => '与集群比较';

  @override
  String get kubeHelmCompareHelp => '只读比较保存的显示字段与活对象。服务器默认值和控制器调节规则没有被评价。';

  @override
  String get kubeHelmCompareMasked => '遮盖的田地不在比较之列。';

  @override
  String get kubeHelmCompareEqual => '可见字段匹配';

  @override
  String get kubeHelmCompareChanged => '发现的差异';

  @override
  String get kubeHelmCompareMissing => '缺少对象';

  @override
  String get kubeHelmTests => 'Helm 测试';

  @override
  String get kubeGitOpsStalled => '已暂停';

  @override
  String get kubeGitOpsStale => '过时';

  @override
  String get kubeStatusUnknown => '尚未检查';

  @override
  String get kubeStatusChecking => '检查中...';

  @override
  String get kubeStatusAvailable => '最后一次检查:可达到';

  @override
  String get kubeStatusForbidden => '上次检查: 访问被拒绝';

  @override
  String get kubeStatusFailed => '上次检查: 失败';

  @override
  String get kubeNavCluster => '分组';

  @override
  String get kubeNavCustom => '自定义资源';

  @override
  String get kubeNavCrdFilter => '过滤 CRD...';

  @override
  String get kubeNavOverview => '概览';

  @override
  String get kubeNavHelmReleases => '紧急释放';

  @override
  String get kubeLabelSelectorHint => '标签: app=api';

  @override
  String get kubeLiveShort => '现场直播';

  @override
  String get kubeSnapshotShort => '抓图';

  @override
  String kubeNoResourcesIn(String namespace) {
    return '无资源$namespace';
  }

  @override
  String get kubePodIp => 'Pod IP 软件';

  @override
  String get kubeHostIp => '主机 IP';

  @override
  String get kubeQos => 'QoS 类';

  @override
  String get kubeAnnotations => '说明';

  @override
  String get kubeContainers => '集装箱';

  @override
  String get kubeControlledBy => '由';

  @override
  String get kubeDetailsSection => '细节';

  @override
  String get kubeUsage => '使用量';

  @override
  String get kubeOperations => '业务';

  @override
  String get kubeSpecSection => '规格';

  @override
  String get kubeCloseInspector => '细节';

  @override
  String get kubeImages => '图像';

  @override
  String get kubeShellShort => '贝壳';

  @override
  String get kubeForwardShort => '端口';

  @override
  String get kubeAiShort => '问大赦国际';

  @override
  String get kubeEditShort => '编辑';

  @override
  String get kubeHistoryShort => '推出';

  @override
  String get kubeRevealShort => '数值';

  @override
  String kubeAiClusterChat(String name) {
    return '集群$name';
  }

  @override
  String get kubeAiGeneralChat => '常规聊天';

  @override
  String get kubeToggleNavigator => '显示或隐藏资源';

  @override
  String get kubeRevision => '订正';

  @override
  String get kubeAppVersion => 'App 版本';

  @override
  String get kubeUpdated => '更新';

  @override
  String get kubeScanHome => '在~/.kube中查找';

  @override
  String get kubeScanOtherFolder => '其他文件夹...';

  @override
  String get kubeImportPasteHint => '或粘贴 kubeconfig YAML 在此';

  @override
  String get kubeEditorConnection => '连接';

  @override
  String get kubeEditorOrganization => '组织';

  @override
  String get kubeEditorSafety => '安全问题';

  @override
  String get kubeReplicasReady => '复制品';

  @override
  String get kubeGitOpsAlertSettings => '通知和提醒';

  @override
  String get kubeFluxCliOpen => '流量CLI';

  @override
  String get kubeOverviewNodes => '节点';

  @override
  String get kubeOverviewPods => '运行中';

  @override
  String get kubeOverviewNamespaces => '命名空间';

  @override
  String get kubeOverviewCpu => 'CPU 组合';

  @override
  String get kubeOverviewMemory => '内存';

  @override
  String get kubeOverviewPodPhases => '按阶段划分';

  @override
  String get kubeNavClusterOverview => '群组概览';

  @override
  String get settingsTerminalFontLabel => '字体';

  @override
  String get settingsTerminalFontSystem => '系统默认';

  @override
  String get settingsTerminalFontCustom => '其他字体...';

  @override
  String get settingsTerminalFontCustomTitle => '终端字体';

  @override
  String get settingsTerminalFontCustomHint => '已安装字体名称, 例如 JetBrains Mono';

  @override
  String get terminalTabRename => '重命名...';

  @override
  String get terminalTabRenameTitle => '标签名称';

  @override
  String get terminalTabResetName => '恢复原名';

  @override
  String get terminalTabDuplicate => '复制';

  @override
  String get terminalTabClose => '关闭';

  @override
  String get terminalTabCloseOthers => '关闭其他标签';

  @override
  String get terminalTabCloseRight => '关闭右侧标签';

  @override
  String get kubeClusterProperties => '属性...';

  @override
  String get kubeMarkAsTest => '标记为测试';

  @override
  String get kubeMarkAsProduction => '标记为生产';

  @override
  String get kubeMarkAsTestConfirm => '生产保障措施(外部确认,默认情况下只读)将关闭。';

  @override
  String get kubeClusterColorMenu => '标签颜色...';

  @override
  String get settingsTerminalTabSubtitle => '标签字幕';

  @override
  String get settingsTerminalTabSubtitleName => '清单上的名字';

  @override
  String get settingsTerminalTabSubtitleAddress => '地址或计算机名称';

  @override
  String get settingsTerminalTabSubtitleOff => '隐藏';

  @override
  String get settingsTerminalTabPlacement => '标签位置';

  @override
  String get settingsTerminalTabPlacementTop => '顶';

  @override
  String get settingsTerminalTabPlacementLeft => '左边';

  @override
  String get settingsTerminalTabPlacementHelp => '左侧: 终端旁边有名称和地址的标签栏。拖动其边缘以更改宽度或折叠到图标上。应用程序侧边栏折叠到它旁边, 直到您扩展它。保存在这个设备上。';

  @override
  String get kubeApplyConflict => '另外一个控制器拥有其中的一些字段, 或者对象已更改。打开“ 获取相冲突字段的所有权 ” 或者重新装入对象。';

  @override
  String get kubeApplyForbidden => '组不允许您更改此对象。';

  @override
  String get kubeApplyInvalid => '服务器拒绝了YAML:检查无法更改的语法和字段(一个Pod的光谱大多是不可改变的).';

  @override
  String kubeApplyOwnedBy(String managers) {
    return '这些字段由$managers 管理。打开“ 获取冲突字段的所有权 ” 来覆盖它们; 控制器可能会将它们更改为返回。';
  }

  @override
  String get kubeApplyRejectedFields => '服务器拒绝了这些字段 :';

  @override
  String kubeApplyPodImmutable(String owner) {
    return '运行中的 Pod 只允许更改图像, 活动Deadline Seconds, 添加容忍度, 标签和说明。编辑其所有者$owner 。';
  }

  @override
  String get shellAiNone => 'AI没有设置——开放设置';

  @override
  String shellAiDevice(String model) {
    return 'AI: 此装置的内置型号 ^ $model';
  }

  @override
  String shellAiLocal(String name, String model) {
    return 'AI: 本地服务器 ${name}_$model';
  }

  @override
  String shellAiApi(String name, String model) {
    return 'AI:通过API${name}_$model(请求离开这台计算机)';
  }

  @override
  String get sshConfigImportButton => '从~/.sh/配置';

  @override
  String get sshConfigImportTitle => '从~/.ssh/配置导入';

  @override
  String get sshConfigImportHelp => '主机条目以其地址,端口,用户,跳转主机和密钥成为主机. Wildcard块(Host *)作为默认应用.';

  @override
  String sshConfigNotFound(String path) {
    return '$path没有找到。';
  }

  @override
  String get sshConfigImportKeys => '从身份文件导入密钥';

  @override
  String get sshConfigImportKeysHelp => '密钥按原样存储在保险库中;加密密钥保留其口令.';

  @override
  String sshConfigVia(String hosts) {
    return '通过$hosts';
  }

  @override
  String get sshConfigProxyCommand => '代理Command 不支持; 主机没有它就导入';

  @override
  String sshConfigKeyFailed(String path) {
    return '未进口密钥$path';
  }

  @override
  String get settingsShortcutsTitle => '键盘快捷键';

  @override
  String get settingsShortcutsSubtitle => '主修版为QQmacOS和CtrlWindows/Linux. App快捷键在终端前处理.';

  @override
  String get settingsShortcutsReset => '恢复默认值';

  @override
  String get settingsShortcutChange => '变动';

  @override
  String settingsShortcutResetOne(String shortcut) {
    return '恢复$shortcut';
  }

  @override
  String get settingsShortcutOff => '关';

  @override
  String settingsShortcutCaptureTitle(String command) {
    return '“$command”的快捷键';
  }

  @override
  String get settingsShortcutCaptureHint => '按下QQ/Ctrl 或 Alt. Esc 的组合取消.';

  @override
  String settingsShortcutConflict(String command) {
    return '“$command”已经使用;该命令将失去它的捷径。';
  }

  @override
  String get settingsShortcutReserved => '系统或终端使用这种组合;它将停止到达终端.';

  @override
  String get settingsShortcutAssign => '指派';

  @override
  String get settingsShortcutDisable => '关机';

  @override
  String get settingsShortcutReservedBadge => '系统或终端重叠';

  @override
  String get kubeMetricsInstall => '安装公尺服务器';

  @override
  String kubeMetricsInstallHelp(String version) {
    return 'Kube系统的基本度量衡服务器$version:服务账户、RBAC、服务、部署和v1beta1.metrics.k8s.io APIService。集群从注册.k8s.io中提取图像。';
  }

  @override
  String get kubeMetricsInsecureTls => 'Kubelets 使用自签名证书 --kubelet-insecure-tls。';

  @override
  String get kubeMetricsInsecureTlsHelp => '本地集群( OrbStack, type, Docker 桌面, 迷你库贝) 需要。关闭 kubelet 证书检查。';

  @override
  String kubeMetricsInstalled(int created, int existing) {
    return '已安装的度量衡服务器$created 已创建, $existing 已存在。度量衡大约在一分钟内出现。';
  }

  @override
  String kubeMetricsInstallDenied(String kind, String name) {
    return '$kind $name : 集群被拒绝。请检查您的权限; 生产集群需要一个打开的写窗口。';
  }

  @override
  String kubeMetricsInstallFailed(String kind, String name) {
    return '$kind $name没有创建.';
  }

  @override
  String kubeMetricsUnknownKind(String kind, String apiVersion) {
    return '该组不提供$kind$apiVersion.';
  }

  @override
  String get navPortForwards => '端口转发';

  @override
  String kubeForwardsOpen(int count) {
    return '前进港:$count';
  }

  @override
  String get kubeForwardCopyAddress => '复制本地地址';

  @override
  String get kubeForwardsEmptyHelp => '从 Pod 或 Service 开始一个: 前进端口。';

  @override
  String get dbTitle => '数据库';

  @override
  String get dbSubtitle => '加密连接; 查询直接从此设备连接。';

  @override
  String get dbUnavailable => '此构建中无法提供数据库支持。';

  @override
  String get dbNewConnection => '新建数据库连接';

  @override
  String get dbImport => '导入连接';

  @override
  String get dbConnectionString => '连接字符串';

  @override
  String get dbImportHelp => '密码留在加密保险库中, 在保存前检查目的地。';

  @override
  String get dbImportPgpass => '选择. pgpass 文件';

  @override
  String get dbImportExpired => '导入审查已过期, 再次导入源。';

  @override
  String get dbEmpty => '还没有数据库连接。';

  @override
  String get dbFailed => '数据库操作失败。请检查连接设置和权限。';

  @override
  String get dbInvalid => '检查突出显示的字段。';

  @override
  String get dbRequired => '要求数';

  @override
  String get dbName => '连接名称';

  @override
  String get dbEngine => '数据库引擎';

  @override
  String get dbHost => '主机';

  @override
  String get dbPort => '端口';

  @override
  String get dbDatabase => '数据库/ Redis 索引';

  @override
  String get dbUsername => '用户名';

  @override
  String get dbPassword => '密码';

  @override
  String get dbSavePassword => '在加密保险库中保存密码';

  @override
  String get dbSavedPassword => '除非替换或删除,否则保留已保存的密码。';

  @override
  String get dbClearPassword => '删除已保存的密码';

  @override
  String get dbProduction => '制作——只读,只在临时窗口写作';

  @override
  String get dbProductionHelp =>
      '生产连接只开放只读。工作空间中的“ 解锁生产 ” 允许在用保险库口令( 或生物鉴别) 确认后写5 - 30分钟; 仍然审查每个更改。优先建立一个数据库账户, 对日常工作有只读权限。';

  @override
  String get dbProd => '方案';

  @override
  String get dbReadOnly => '仅读';

  @override
  String get dbWriteAllowed => '此会话允许写入';

  @override
  String get dbAllowWrite => '允许为此会话写入';

  @override
  String get dbAllowWriteHelp => '只有这个非制作会话才允许写入。危险询问仍需确认。';

  @override
  String get dbRevokeWrite => '返回只读模式';

  @override
  String get dbPrivileges => '检查权限';

  @override
  String get dbPrivilegesWarning => '此账户可能有写权限。使用只读的角色来制作。';

  @override
  String get dbServerReadOnly => '服务器只读保护已激活。';

  @override
  String get dbRoleReadOnly => '账户的只读角色被验证.';

  @override
  String get dbGuardOnly => '本地查询保护活动; 服务器只读权限未验证。';

  @override
  String get dbTls => 'TLS保护';

  @override
  String get dbTlsVerify => '校验证书和主机名';

  @override
  String get dbTlsRequire => '需要加密';

  @override
  String get dbTlsPrefer => '优先加密';

  @override
  String get dbTlsDisable => '禁用TLS';

  @override
  String get dbTlsWarning => '此模式不会验证数据库服务器身份。优先证书和主机名验证。';

  @override
  String get dbTlsCa => '证书权限( PEM)';

  @override
  String get dbTlsServerName => 'TLS 服务器名称覆盖';

  @override
  String get dbClientCertificate => '客户端证书( PEM)';

  @override
  String get dbPrivateKey => '客户端私钥( PEM)';

  @override
  String get dbAuthMechanism => '认证机制';

  @override
  String get dbAuthDefault => '默认';

  @override
  String get dbRoute => '连接路径';

  @override
  String get dbDirect => '直接';

  @override
  String get dbSsh => 'SSH主机';

  @override
  String get dbJump => 'SSH 跳跃简介';

  @override
  String get dbProxy => '代理服务器';

  @override
  String get dbRouteHelp => 'TLS验证也使用通过SSH路由的原始数据库主机名.';

  @override
  String get dbGroup => '组';

  @override
  String get dbUngrouped => '未分组';

  @override
  String get dbTags => '标记, 用逗号分隔';

  @override
  String get dbColor => '颜色';

  @override
  String get dbLimits => '限制';

  @override
  String get dbMaxRows => '初始结果行';

  @override
  String get dbStatementTimeout => '查询超时秒';

  @override
  String get dbConnectTimeout => '连接超时, 秒';

  @override
  String get dbMemoryLimit => '结果内存, MiB';

  @override
  String get dbOptions => '驱动程序设置';

  @override
  String get dbApplicationName => '应用程序名称';

  @override
  String get dbSearchPath => '搜索路径, 用逗号分隔';

  @override
  String get dbCharset => '字符集 :';

  @override
  String get dbMongoAuthSource => '认证数据库';

  @override
  String get dbMongoReplicaSet => '复制集';

  @override
  String get dbMongoSrv => '使用DNSSRV发现';

  @override
  String get dbMongoReadPreference => '阅读偏好';

  @override
  String get dbMongoReadHelp => '阅读偏好选择复制件; 它不禁止写作。';

  @override
  String get dbTestConnection => '测试连接';

  @override
  String get dbTestSuccess => '连接成功。';

  @override
  String get dbDisconnect => '断开连接';

  @override
  String get dbRefresh => '刷新';

  @override
  String get dbDeleteHelp => '删除此已保存的连接吗 ? 活动会话会关闭。';

  @override
  String get dbLocked => '解锁保险库访问数据库连接。';

  @override
  String get dbTree => '结构';

  @override
  String get dbDdl => '显示 DDL';

  @override
  String get dbFirstRows => '预览前100行';

  @override
  String get dbInsertName => '插入标识符';

  @override
  String get dbInspect => '对象细节';

  @override
  String get dbSampleSchema => '从样本推断出计划';

  @override
  String get dbSampleHelp => '这里读取所选收藏中的文档来推断字段类型。没有数据发送到 AI。';

  @override
  String get dbLoadMore => '装入更多';

  @override
  String get dbEditor => '查询编辑器';

  @override
  String get dbNewQuery => '新建查询';

  @override
  String get dbRun => '运行';

  @override
  String get dbRunCurrent => '当前声明';

  @override
  String get dbRunSelection => '选择';

  @override
  String get dbRunScript => '整个脚本';

  @override
  String get dbCancelQuery => '取消查询';

  @override
  String get dbFormat => '格式查询';

  @override
  String get dbComment => '切换行注释';

  @override
  String get dbFindReplace => '查找和替换';

  @override
  String get dbReplace => '替换';

  @override
  String get dbReplaceAll => '全部替换';

  @override
  String get dbVariables => '查询变量';

  @override
  String get dbVariablesHelp => '数值被绑定为本地参数,从未被调和到查询中.';

  @override
  String get dbValueType => '数值类型';

  @override
  String get dbValue => '数值';

  @override
  String get dbReviewQuery => '审查查询';

  @override
  String get dbQueryBlocked => '此查询被连接策略阻断。使用具有适当权限的非生产连接。';

  @override
  String get dbQueryDangerous => '此查询可以更改或删除数据。审查查询并输入保险库口令。';

  @override
  String get dbReviewExpired => '复核过期。再次运行查询以获取新的复核。';

  @override
  String get dbResults => '结果';

  @override
  String get dbGrid => '网格';

  @override
  String get dbRecords => '记录';

  @override
  String get dbNoResults => '运行查询以查看结果。';

  @override
  String get dbNull => 'NULL 函数';

  @override
  String get dbTruncated => '达到结果限制。导出以流出全部结果。';

  @override
  String get dbCancelled => '查询已取消。';

  @override
  String get dbRunning => '查询运行中...';

  @override
  String get dbMessages => '服务器消息';

  @override
  String get dbShowValue => '显示全部值';

  @override
  String get dbPinColumn => '键列';

  @override
  String get dbUnpinColumn => '撤销列';

  @override
  String get dbColumns => '列数';

  @override
  String get dbCopyCell => '复制单元格';

  @override
  String get dbCopyRow => '复制行';

  @override
  String get dbCopySelection => '复制选择';

  @override
  String get dbCopyResult => '复制已装入的结果';

  @override
  String get dbSensitiveClipboard => '剪贴板根据您的安全设置自动清除。';

  @override
  String get dbExport => '导出结果';

  @override
  String get dbExportFormat => '格式';

  @override
  String get dbExportHeader => '包含列名称';

  @override
  String get dbExportBom => 'UTF-8字节顺序标记(BOM)';

  @override
  String get dbExportDelimiter => '分隔符';

  @override
  String get dbExportQuote => '引用字符';

  @override
  String get dbExportHelp => '导出全部结果到选中的文件。结果可能包含敏感数据。';

  @override
  String get dbExportDone => '导出完成。';

  @override
  String get dbHistory => '查询历史';

  @override
  String get dbFavorites => '爱好';

  @override
  String get dbAllQueries => '所有查询';

  @override
  String get dbSuccessful => '成功';

  @override
  String get dbWithErrors => '有错误';

  @override
  String get dbHistoryLocal => '本地加密历史';

  @override
  String get dbHistorySync => '加密同步';

  @override
  String get dbHistoryDisabled => '历史已禁用';

  @override
  String get dbClearHistory => '清除查询历史';

  @override
  String get dbClearHistoryHelp => '删除此连接的保存的查询历史 ?';

  @override
  String get dbSaveSnippet => '另存为片段';

  @override
  String get dbSnippetName => '片段名称';

  @override
  String get dbExplain => '发言';

  @override
  String get dbExplainAnalyze => '用 ANALYZE 执行';

  @override
  String get dbExplainAnalyzeHelp => 'ANALYZE 执行查询以收集测量数据。仅用于非生产连接。';

  @override
  String get dbTrace => '跟踪查询';

  @override
  String get dbTraceHelp => '查询有一次追踪: 每个节点记录其步骤和系统_跟踪中的延迟。仅用于非生产连接。';

  @override
  String get dbEditCell => '编辑单元格';

  @override
  String get dbApplyEdits => '审查待编辑';

  @override
  String get dbDiscardEdits => '丢弃更改';

  @override
  String get dbCellEditsHelp => '审查参数化的 UPDATE 语句。修改应用在一个交易中;失败将它卷回。';

  @override
  String get dbAi => '大赦国际助理';

  @override
  String get dbAiQuestion => '说明你需要什么';

  @override
  String get dbAiProvider => 'AI 供应商';

  @override
  String get dbAiCompose => '编写查询';

  @override
  String get dbAiExplain => '解释查询';

  @override
  String get dbAiFix => '用人工智能修复';

  @override
  String get dbAiOptimize => '优化与AI';

  @override
  String get dbAiTranslate => '翻译方言';

  @override
  String get dbAiSnippet => '创建参数化片段';

  @override
  String get dbAiExamples => '示例查询';

  @override
  String get dbAiResult => '解释所选行';

  @override
  String get dbAiDraft => '大赦国际草案——接受之前的审查';

  @override
  String get dbAiAccept => '接受草稿';

  @override
  String get dbAiReject => '拒绝';

  @override
  String get dbAiRefine => '完善';

  @override
  String get dbAiConsent => '审查大赦国际的背景';

  @override
  String get dbAiSchema => '允许设置元数据';

  @override
  String get dbAiRows => '允许明确选择结果行';

  @override
  String get dbAiSample => '允许样本位( 最多 20 行)';

  @override
  String get dbAiReadonly => '允许只读运行_ (最多50行)';

  @override
  String get dbAiConsentHelp => 'Schema和行数据需要单独的同意。工具调用只读,以下显示,不能应用更改。';

  @override
  String get dbAiExternal => '此提供者是外部的。在本地发送并恢复标识之前, 严格隐私将替换标识符。';

  @override
  String get dbAiLocal => '这个供应商在当地运行。';

  @override
  String get dbAiContextLimit => '上下文中的最大计划对象';

  @override
  String get dbAiTools => '发送到模型的背景';

  @override
  String get dbAiNoExecute => '接受草稿永远不会执行。在审查后使用。';

  @override
  String get dbAiProdDdl => 'DDL生产建议只是建议,不能执行。';

  @override
  String get dbAiRevoke => '撤销大赦国际的同意';

  @override
  String get dbBackgroundIos => '在iOS上,一个运行中的查询在背景的30秒后取消.';

  @override
  String get dbCloseRunning => '关闭此查询并取消其活动请求 ?';

  @override
  String get dbOperationFailed => '数据库操作失败。请检查连接和权限, 然后重试。';

  @override
  String get dbTreeSearch => '搜索已装入的对象';

  @override
  String get dbInvalidValue => '检查值及其类型。';

  @override
  String get dbComplete => '完整查询';

  @override
  String get dbBinaryPreview => '十六进制预览( 前 4096 字节)';

  @override
  String get dbAiNewConversation => '新对话';

  @override
  String get dbAiHistoryMessages => '请求中的历史消息';

  @override
  String get dbVerifiedReadOnly => '服务器端只读保护被验证。';

  @override
  String get dbRoleCanWrite => '此数据库角色具有写入权限。使用专用的只读角色来制作。';

  @override
  String get dbResultStale => '应用更改。再次运行查询以刷新结果。';

  @override
  String get dbAllowWritesHelp => '允许为这个非生产会话写入吗 ? 每个更改的查询仍然需要审查。当连接关闭时此权限结束。';

  @override
  String get dbCheckPrivileges => '检查权限';

  @override
  String get dbAllowWrites => '允许写入';

  @override
  String get dbUnverifiedReadOnly => '服务器端只读保护是未验证的。使用专用的只读数据库角色。';

  @override
  String dbElapsed(int ms) {
    return '$ms分钟)';
  }

  @override
  String get dbQueryBlockedTitle => '查询被屏蔽';

  @override
  String get dbScanInsteadOfKeys => '关键设备扫描每个密钥, 可以拖住服务器。使用 SCAN 来逐页读取密钥。';

  @override
  String get dbOpenSuggestion => '插入到编辑器中';

  @override
  String get dbPlanByTime => '条子显示每个步骤在执行时间中自己的份额.';

  @override
  String get dbPlanByCost => '酒吧显示每个步骤自己在估计费用中的份额.';

  @override
  String dbPlanTime(String ms) {
    return '$ms分钟)';
  }

  @override
  String dbPlanRows(int count) {
    return '$count行数';
  }

  @override
  String dbPlanCost(String cost) {
    return '费用$cost';
  }

  @override
  String get dbPlanTree => '计划树';

  @override
  String get dbPlanJson => 'JSON';

  @override
  String get dbErrorTls => 'TLS失败:服务器不提供TLS,或者其证书没有通过检查。对于一个端口前方或本地数据库选择TLS“偏好”或“残疾”;对于远程服务器检查证书和服务器名称。';

  @override
  String get dbErrorAuth => '拒绝签名:检查用户名,密码和数据库名称.';

  @override
  String get dbErrorConnection => '无法到达服务器: 检查地址, 端口以及端口前方或隧道正在运行。';

  @override
  String get dbErrorTimeout => '服务器没有及时回复。请检查地址和连接超时。';

  @override
  String get dbErrorRoute => '所选的路由不可用: 请检查SSH 主机或跳转配置。';

  @override
  String get dbTlsLocalHint => '本地地址:TLS设置为\"偏好\"(一个前港通常没有TLS).';

  @override
  String get dbTransportVerified => 'TLS(已核实)';

  @override
  String get dbTransportUnverified => 'TLS(证书未经核实)';

  @override
  String get dbTransportTunnel => '编号TLS——由隧道保护.';

  @override
  String get dbTransportPlaintext => '编号TLS';

  @override
  String get dbTlsPreferHelp => 'TLS,如果服务器支持它;没有TLS,只能通过隧道或在这个计算机上.';

  @override
  String get dbTlsRequireHelp => '不验证证书的加密。';

  @override
  String get dbTlsAllowUnbound => '允许不装订TLS 的登录( 不安全)';

  @override
  String get dbTlsAllowUnboundHelp => '仅针对拥有md5或密码认证的服务器或没有SCRAM-SHA-256-PLUS的服务器,没有经过核实的证书,截取器就可以获得密码.';

  @override
  String get dbErrorTlsUnavailable => '服务器不支持 TLS。不加密连接会以清晰的文本发送密码。如果可以接受, 明确选择“ Disable TLS ” 。';

  @override
  String get dbErrorChannelBinding =>
      '服务器没有将密码绑定在这个TLS会话(SCRAM-SHA-256-PLUS)上,证书也没有被校验:一个截取器可以获得密码. 选择\"验证证书和主机名\",在服务器上使用SCRAM,或者允许在TLS保护中不装订TLS的签入.';

  @override
  String get dbErrorCleartext => '服务器在未加密的网络连接上请求清晰文本的密码。启用TLS 或者通过隧道连接。';

  @override
  String get colorPickerCustom => '自定义颜色';

  @override
  String get kubeConnectDatabase => '连接到数据库';

  @override
  String kubeConnectDatabaseHelp(String engine) {
    return '将$engine端口转发到本地主机并在数据库中打开';
  }

  @override
  String get dockerTitle => 'Docker';

  @override
  String get dockerSubtitle => '本计算机或保存SSH主机上的容器。';

  @override
  String get dockerUnavailable => 'Docker 无法使用';

  @override
  String get dockerUnavailableHelp => '应用程序的这种构建无法到达引擎Docker.';

  @override
  String get dockerSearchHosts => '搜索主机';

  @override
  String get dockerRefresh => '刷新';

  @override
  String get dockerHostsFailed => '无法装入主机列表。';

  @override
  String get dockerNoHosts => '没有主机';

  @override
  String get dockerNoHostsHelp => 'Docker没有在这个计算机上找到,还没有保存SSH主机.';

  @override
  String get dockerLocalHost => '这台电脑';

  @override
  String get dockerLocalHostHelp => '本地Docker发动机';

  @override
  String get dockerConnect => '连接';

  @override
  String get dockerConnecting => '正在连接...';

  @override
  String get dockerReconnect => '重新连接';

  @override
  String get dockerPermissionDeniedTitle => '无法访问Docker套接字';

  @override
  String dockerPermissionDenied(String host, String group) {
    return '那个 SSH 打开 $host 无权享有 Docker 套接字。将用户添加到“$group“主机和重联上的组 :';
  }

  @override
  String get dockerPermissionDeniedNote => '自己在主机上运行; 应用程序从不运行。新组适用于新的SSH 会话。';

  @override
  String get dockerCopyCommand => '复制命令';

  @override
  String get dockerNotRunningTitle => 'Docker没有运行';

  @override
  String dockerNotRunning(String host) {
    return 'Docker没有运行或不安装在$host上. 在那里启动Docker服务并重新连接.';
  }

  @override
  String get dockerConnectionFailedTitle => '连接失败';

  @override
  String dockerConnectionFailed(String host) {
    return '无法连接到 $host 。';
  }

  @override
  String get dockerRequestFailed => 'Docker 请求失败';

  @override
  String get dockerRetry => '重试';

  @override
  String get dockerDisconnect => '断开连接并选择另一个主机';

  @override
  String dockerEngineVersion(String version) {
    return '发动机$version';
  }

  @override
  String dockerRunningOfTotal(int running, int total) {
    return '$running $total 运行中';
  }

  @override
  String get dockerForwards => '前进端口';

  @override
  String get dockerFilterContainers => '通过名称或图像过滤';

  @override
  String get dockerNoContainers => '无容器';

  @override
  String get dockerNoContainersHelp => '从这个引擎开始的集装箱就在这里出现.';

  @override
  String get dockerNoMatches => '没有匹配的容器';

  @override
  String get dockerContainerGone => '容器已经不存在';

  @override
  String get dockerStandalone => '集装箱';

  @override
  String get dockerCompose => '编曲';

  @override
  String dockerGroupRunning(int running, int total) {
    return '$running/$total运行';
  }

  @override
  String get dockerStateCreated => '已创建';

  @override
  String get dockerStateRunning => '运行';

  @override
  String get dockerStatePaused => '暂停';

  @override
  String get dockerStateRestarting => '重新启动';

  @override
  String get dockerStateRemoving => '删除';

  @override
  String get dockerStateExited => '已退出';

  @override
  String get dockerStateDead => '死了吗?';

  @override
  String get dockerStateUnknown => '未知';

  @override
  String get dockerHealthHealthy => '卫生';

  @override
  String get dockerHealthUnhealthy => '身体不健康 病态';

  @override
  String get dockerHealthStarting => '开始';

  @override
  String get dockerActionStart => '开始';

  @override
  String get dockerActionStop => '停下来';

  @override
  String get dockerActionRestart => '重新开始';

  @override
  String get dockerActionPause => '暂停';

  @override
  String get dockerActionUnpause => '继续';

  @override
  String get dockerActionKill => '杀';

  @override
  String get dockerActionRemove => '移除';

  @override
  String dockerKillTitle(String name) {
    return '杀$name?';
  }

  @override
  String get dockerKillHelp => '集装箱的加工过程立即停止,没有机会清洁关闭.';

  @override
  String dockerRemoveTitle(String name) {
    return '删除$name?';
  }

  @override
  String get dockerRemoveHelp => '删除容器会清除其可写层。请选择要一并删除的卷或网络。';

  @override
  String get dockerRemovalBindMounts => '挂载到容器内的主机文件';

  @override
  String get dockerRemovalShared => '其他容器正在使用';

  @override
  String get dockerRemovalExternal => '由外部管理';

  @override
  String get dockerRemovalDefaultNetwork => 'Docker 内置网络';

  @override
  String get dockerRemovalBindKept => '主机文件将保留';

  @override
  String get dockerRemovalUnverified => '无法确认此资源是否仅由该容器使用';

  @override
  String get dockerRemovalStopFirst => '删除前停止此容器';

  @override
  String get dockerRemovalVolumeWarning => '所选卷中的所有数据都将被永久删除。';

  @override
  String get dockerRemovalResult => '删除结果';

  @override
  String get dockerRemoveContainer => '删除容器…';

  @override
  String get dockerRemovalRemoved => '容器已删除';

  @override
  String get dockerRemovalStopped => '容器已停止，但未确认是否已删除。';

  @override
  String get dockerRemovalKept => '未确认是否已删除容器。';

  @override
  String get dockerRemovalResourceRemoved => '已删除';

  @override
  String dockerRemovalResourceSkipped(String reason) {
    return '已跳过。$reason';
  }

  @override
  String dockerRemovalResourceFailed(String reason) {
    return '删除失败。$reason';
  }

  @override
  String get dockerRemovalChanged => '容器或其资源已发生变化。请重新打开删除对话框以查看当前状态。';

  @override
  String get dockerRemovalAutoRemove => 'Docker 会在容器停止时自动删除该容器及其匿名卷。在此设置下无法选择要保留的卷。';

  @override
  String get dockerRemovalMissing => '该资源已不存在';

  @override
  String get dockerRemovalInUse => 'Docker 报告该资源仍在使用。';

  @override
  String get dockerRemovalCancelled => '操作已中断。请刷新以查看当前状态。';

  @override
  String get dockerLogs => '日志';

  @override
  String get dockerTerminal => '终端';

  @override
  String get dockerForwardPort => '前进端口';

  @override
  String get dockerDetails => '细节';

  @override
  String get dockerCopyId => '复制标识';

  @override
  String get dockerContainerId => '集装箱标识';

  @override
  String get dockerOverview => '概览';

  @override
  String get dockerInspectJson => 'JSON';

  @override
  String get dockerCopyJson => '复制JSON';

  @override
  String get dockerImage => '图像';

  @override
  String get dockerStatus => '状态';

  @override
  String get dockerHealth => '卫生';

  @override
  String get dockerCreated => '已创建';

  @override
  String get dockerStarted => '开始';

  @override
  String get dockerCommand => '命令';

  @override
  String get dockerRestartCount => '重新启动';

  @override
  String get dockerComposeProject => '构建项目';

  @override
  String get dockerComposeService => '编译服务';

  @override
  String get dockerPorts => '港口';

  @override
  String get dockerNetworks => '网络';

  @override
  String get dockerMounts => '挂载';

  @override
  String get dockerLabels => '标签';

  @override
  String get dockerEnvironment => '环境';

  @override
  String get dockerLogsPause => '暂停';

  @override
  String get dockerLogsResume => '继续';

  @override
  String get dockerLogsFollow => '跟着';

  @override
  String get dockerLogsTimestamps => '时间戳';

  @override
  String get dockerLogsCopy => '复制显示的行';

  @override
  String get dockerLogsClear => '清除';

  @override
  String get dockerLogsFind => '在日志中查找';

  @override
  String dockerLogsMatches(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count 匹配数',
      one: '$count 比赛',
      zero: ' 无匹配',
    );
    return '$_temp0';
  }

  @override
  String get dockerLogsPaused => '暂停: 保留新行直到恢复';

  @override
  String dockerLogsTrimmed(int limit) {
    return '仅保留最后一行$limit';
  }

  @override
  String get dockerLogsFailed => '日志流以错误停止';

  @override
  String get dockerLogsWaiting => '正在等待日志行...';

  @override
  String get dockerNoLogs => '无日志行';

  @override
  String get dockerForwardHelpSsh => '当地一个港口通过SSH连接宿主的连接线传送到集装箱港口.';

  @override
  String get dockerForwardHelpLocal => '在这个计算机上,一个本地端口被传送到集装箱端口。';

  @override
  String get dockerForwardContainerPort => '集装箱港';

  @override
  String get dockerForwardLocalPort => '本地端口';

  @override
  String get dockerForwardLocalHelp => '0 选择自由端口';

  @override
  String get dockerForwardStart => '开始前进';

  @override
  String get dockerForwardCreated => '前方正在运行,它列在端口前方面板中.';

  @override
  String get dockerForwardCopyAddress => '复制地址';

  @override
  String get dockerForwardStop => '停止前进';

  @override
  String get dockerForwardRunning => '运行';

  @override
  String get dockerForwardFailed => '失败';

  @override
  String get dockerForwardStopped => '已停止';

  @override
  String dockerForwardTraffic(int connections, String sent, String received) {
    return '$connections 个连接 · ↑ $sent · ↓ $received';
  }

  @override
  String get dockerNoForwards => '无前进端口';

  @override
  String get dockerNoForwardsHelp => '在运行中的集装箱上使用“前进端口”。';

  @override
  String get dockerTerminalTitle => '集装箱码头';

  @override
  String get dockerShell => '贝壳';

  @override
  String get dockerAutoShell => '自动检测( bash、灰或sh)';

  @override
  String get dockerOpenShell => '打开终端';

  @override
  String get dockerTerminalFailed => '无法打开容器中的终端。';

  @override
  String get dockerSocketForwardingDisabled =>
      'SSH服务器不允许转发Docker套接字,且没有DockerCLI. Sshd_config中设置\"AllowStream LocalForwarding yes\",或在主机上安装docker CLI.';

  @override
  String get dockerStateFilter => '集装箱状态';

  @override
  String dockerFilterAll(int count) {
    return '总计$count';
  }

  @override
  String dockerFilterRunning(int count) {
    return '运行$count';
  }

  @override
  String dockerFilterStopped(int count) {
    return '已停止$count';
  }

  @override
  String get dockerRevealSecrets => '显示秘密';

  @override
  String get dockerHideSecrets => '隐藏秘密';

  @override
  String dockerRevealHelp(String name) {
    return '输入保险库口令, 以显示$name 环境中的密码和令牌。复制值会自动从剪贴板中清除。';
  }

  @override
  String get dockerVaultPassphrase => '保险库口令';

  @override
  String get dockerWrongPassphrase => '保险库密码错误。';

  @override
  String get dockerMountType => '类型';

  @override
  String get dockerMountSource => '来源';

  @override
  String get dockerMountDestination => '目标';

  @override
  String get dockerMountMode => '模式';

  @override
  String get dockerMountReadOnly => '罗';

  @override
  String get dockerMountReadWrite => '转';

  @override
  String get dockerView => '显示';

  @override
  String get dockerVolumes => '卷数';

  @override
  String get dockerVolumesOther => '其他各卷';

  @override
  String dockerVolumeCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count册)', one: '1册');
    return '$_temp0';
  }

  @override
  String get dockerVolumeUnused => '未使用';

  @override
  String get dockerNoVolumes => '引擎上没有卷';

  @override
  String get workspaceForwardsEmptyTitle => '没有活动的端口转发';

  @override
  String get workspaceForwardsEmptyHelp => '要转发端口，请打开 Kubernetes Pod 或 Docker 容器。';

  @override
  String shellAiDeviceIdle(String model) {
    return 'AI:内置型号未运行(按请求启动) ^ $model';
  }

  @override
  String shellAiDeviceStarting(String model) {
    return 'AI: 内置型号开始_$model';
  }

  @override
  String shellAiDeviceError(String model) {
    return 'AI:内置型号启动失败 ^ $model.';
  }

  @override
  String get dbEditorHint => '写一个查询... 运行它';

  @override
  String dbRowCount(String count) {
    return '行:$count';
  }

  @override
  String dbRowsAffected(int count) {
    return '受影响:$count';
  }

  @override
  String get dbStatusReady => '准备';

  @override
  String dbRunStatementHelp(String shortcut) {
    return '运行选择, 或光标下的语句 $shortcut 。';
  }

  @override
  String dbRunScriptHelp(String shortcut) {
    return '按语句运行整个脚本语句$shortcut.';
  }

  @override
  String get dbFolderTables => '表格';

  @override
  String get dbFolderViews => '视图';

  @override
  String get dbFolderFunctions => '职能';

  @override
  String get dbFolderSequences => '顺序';

  @override
  String get dbFolderDictionaries => '词典';

  @override
  String get dbFolderColumns => '列数';

  @override
  String get dbFolderIndexes => '索引';

  @override
  String get dbFolderConstraints => '制约因素';

  @override
  String get dbFolderTriggers => '触发器';

  @override
  String get dbFolderProjections => '预测';

  @override
  String get dbCollapseAll => '全部折叠';

  @override
  String dockerConnectDatabaseHelp(String engine) {
    return '在数据库中打开此容器的数据库$engine';
  }

  @override
  String dbRouteKubernetes(String name) {
    return 'Kubernetes : $name';
  }

  @override
  String get dbContainerPort => '集装箱港';

  @override
  String get dbKubeTarget => '转发至';

  @override
  String get dbKubeTargetName => '对象名称';

  @override
  String get dbKubeRouteHelp => '每一个连接上都恢复前端,或者重复使用运行中的连接。服务和部署使用一个准备的吊舱,因此,吊舱的连接能够继续重新启动。';

  @override
  String get dbErrorKubeUnreachable => '无法到达Kubernetes 集群以恢复端口前方: 请检查上下文、网络和您的登录。';

  @override
  String get dbErrorKubeTarget => '未找到端口前方的吊舱: 请检查连接路由中的命名空间和对象名称。';

  @override
  String get dbErrorKubeDenied => '该集群拒绝进入舱位或其前端。';

  @override
  String get dbErrorKubeForward => '无法启动端口前方: 停止未使用的前进并再次尝试。';

  @override
  String get dbRouteKubeHint => 'Kubernetes 向前端, 连接后恢复';

  @override
  String dbRouteTunnel(String name) {
    return '隧道:$name';
  }

  @override
  String get dbRouteDocker => 'Docker集装箱';

  @override
  String get dbDockerEngine => 'Docker发动机';

  @override
  String get dbDockerContainer => '容器名称';

  @override
  String get dbDockerRouteHelp => '每一个连接处都恢复了往前到容器的端口,或者重用一个运行中的容器,容器是用名字找到的,所以重新创建的容器仍然有效.';

  @override
  String get dbTunnelRouteHelp => '保存的隧道在未运行时启动连接。地址和端口是隧道目标。';

  @override
  String dbUseTunnel(String name) {
    return '使用已保存的隧道“$name”';
  }

  @override
  String get dbErrorTunnel => '无法启动保存的隧道: 请检查它 SSH 位于隧道的主持人。';

  @override
  String get dbErrorTunnelKind => '只有本地端口前方隧道可以承载数据库连接.';

  @override
  String get dbErrorDockerUnreachable => '无法到达Docker引擎恢复端口前方:检查它是否在运行和SSH主机.';

  @override
  String get dbErrorDockerContainer => '未找到或未运行的容器: 请检查连接路线中的名称。';

  @override
  String get dbErrorDockerForward => '无法启动Docker 端口前方: 停止未使用的前进并再次尝试。';

  @override
  String get dockerOpenInSftp => '开放于SFTP';

  @override
  String get dockerVolumeInVm => '此卷存储在Docker虚拟机内部,因此其文件夹无法从这台计算机打开.';

  @override
  String get dbRouteTunnelHint => '已保存SSH 隧道, 已连接';

  @override
  String get dbRouteDockerHint => 'Docker 向前端口, 连接后恢复';

  @override
  String get dockerComposeActions => '项目行动';

  @override
  String get dockerComposeStartAll => '全部开始';

  @override
  String get dockerComposeStopAll => '别闹了';

  @override
  String get dockerComposeRestartAll => '重新启动全部';

  @override
  String get dockerComposeViewYaml => '视图 YAML';

  @override
  String get dockerComposeEditYaml => '编辑 YAML';

  @override
  String dockerComposeStopAllTitle(String project) {
    return '停止所有$project的集装箱?';
  }

  @override
  String dockerComposeRestartAllTitle(String project) {
    return '重开所有$project的容器?';
  }

  @override
  String get dockerComposeRemove => '移除 Compose 项目…';

  @override
  String dockerComposeRemoveTitle(String project) {
    return '移除项目 $project？';
  }

  @override
  String get dockerComposeRemoveHelp => '将停止列表中正在运行的容器，并移除其不再使用的项目网络。主机上的 Compose 文件和绑定挂载会保留。';

  @override
  String get dockerComposeRemoveVolumes => '同时移除这些容器不再使用的卷';

  @override
  String dockerComposeRemoveSummary(String project, int removed, int total) {
    return '已移除项目 $project 的容器：$removed/$total。';
  }

  @override
  String dockerComposeBulkHelp(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count 集装箱受到影响。', one: '1个集装箱受到影响。');
    return '$_temp0';
  }

  @override
  String dockerComposeBulkFailed(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: ' 该操作失败了$count集装箱。', one: '1个容器的操作失败。');
    return '$_temp0';
  }

  @override
  String dockerComposeFilesTitle(String project) {
    return '编写文件:$project';
  }

  @override
  String dockerComposeWorkingDir(String path) {
    return '项目文件夹:$path';
  }

  @override
  String dockerComposeEnvFiles(String files) {
    return '环境文件:$files';
  }

  @override
  String get dockerComposeFiles => '文件';

  @override
  String dockerComposeUnlockHelp(String project) {
    return '编译文件可以包含密码,并决定主机运行的内容。输入库口令以编辑$project的文件。';
  }

  @override
  String get dockerComposeUnlockExpired => '编辑权限已过期。请再次输入保险库口令以保存。';

  @override
  String get dockerComposeUnlock => '允许编辑';

  @override
  String get dockerComposeSaveApply => '保存并应用';

  @override
  String get dockerComposeSaveOnly => '不应用保存';

  @override
  String get dockerComposeValidating => '正在检查主机上的配置...';

  @override
  String get dockerComposeInvalid => '配置有错误, 没有保存。';

  @override
  String dockerComposeConflict(String path) {
    return '$path 主机打开后在主机上更改了, 没有保存 : 复制您的更改, 然后打开文件。';
  }

  @override
  String dockerComposeWriteDenied(String path) {
    return '没有写权限$path 或其文件夹。文件没有更改。';
  }

  @override
  String dockerComposeWriteFailed(String path) {
    return '无法写入$path 。文件没有更改。';
  }

  @override
  String get dockerComposeMissing => '主机没有编曲命令行工具, 因此无法检查更改。没有保存任何内容。';

  @override
  String dockerComposeRunFailed(String detail) {
    return '检查无法运行( $detail) 。没有保存。';
  }

  @override
  String dockerComposeSavedBackups(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '保存。以前的版本保存为:', one: '已保存。上一版本保留为 :');
    return '$_temp0';
  }

  @override
  String get dockerComposeSavedNotApplied => '运行中的容器仍然使用之前的配置.';

  @override
  String get dockerComposeApplying => '正在应用更改...';

  @override
  String get dockerComposeApplied => '进行了修改。';

  @override
  String dockerComposeApplyFailed(int code) {
    return '应用失败( 退出代码 $code ) 。';
  }

  @override
  String get dockerComposeApplyError => '应用失败。';

  @override
  String get dockerComposeOutput => '输出';

  @override
  String get dockerComposeDiscardTitle => '放弃改变?';

  @override
  String get dockerComposeDiscardHelp => '编辑的文件尚未保存。';

  @override
  String get dockerComposeDiscard => '丢弃';

  @override
  String get dockerComposeFileNotFound => '此文件在主机上已不存在。';

  @override
  String get dockerComposeFileDenied => '没有读取此文件的权限: 它可能属于另一个用户, 如 root。';

  @override
  String get dockerComposeFileTooLarge => '该文件大于 1 MB 且未显示。';

  @override
  String get dockerComposeFileNotText => '文件不是UTF-8文本.';

  @override
  String get dockerComposeFileReadFailed => '无法读取文件。';

  @override
  String get dockerComposeProjectGone => '该项目不再有集装箱。';

  @override
  String get dockerComposeFilesUnknown => '该项目的容器没有记录创建文件的由何人组成。';

  @override
  String get sharingDbConnection => '数据库连接';

  @override
  String get sharingDbUnsupported => '此服务器不支持共享数据库连接。';

  @override
  String get sharingDbSnapshotChanged => '如果此连接的设置或证书已改变, 请再次审查。';

  @override
  String get sharingDbExclude => '只有连接设置是默认共享的。密码、客户端密钥、组、查询历史和 AI 设置仍保留在这个设备上。';

  @override
  String get sharingDbIncludeCredentials => '包含已保存的密码和客户端密钥';

  @override
  String get sharingDbCredentialWarning => '收件人可以使用这些证书访问数据库。仅与您信任的已核实的设备共享。取消该份额不会取消已被收件人复制的证书: 更改数据库服务器上的密码。';

  @override
  String get sharingDbCredentialsIncluded => '包含密码或客户端密钥; 值保持隐藏。';

  @override
  String get sharingDbCredentialsExcluded => '不包含密码和客户端密钥。';

  @override
  String get sharingDbRouteRelay => '此连接通过一个SSH 主机, 跳跃配置文件, 代理或隧道, 无法共享。收件人选择自己的路径。';

  @override
  String get sharingDbRouteKubernetes => '此连接来自一个Kubernetes 工作量。收件人选择自己的Kubernetes 上下文; 命名空间和工作量保留。';

  @override
  String get sharingDbRouteDocker => '此连接来自一个Docker 容器。收件人选择自己的Docker 引擎; 保存容器名称。';

  @override
  String get sharingDbCopyHelp => '保存此连接的个人副本到数据库。选择自己的路由。密码和客户端密钥只随您的明确选择而导入。';

  @override
  String get sharingDbRouteRequired => '此连接无法直接到达。请选择在此设备上保存的兼容路由。';

  @override
  String get sharingDbNoRoute => '此设备没有保存兼容的路由。先添加一个。';

  @override
  String get sharingDbImportCredentials => '导入包含的密码和客户端密钥';

  @override
  String get sharingDbRouteKind => '路线';

  @override
  String get sharingDbRouteKindRelay => 'SSH,跳跃配置,代理或隧道';

  @override
  String get sharingDbWorkload => '工作量';

  @override
  String get sharingDbContainer => '集装箱';

  @override
  String get dbSharedTitle => '共享连接';

  @override
  String get dbSharedBadge => '共享';

  @override
  String get dbSharedOwned => '与你共享';

  @override
  String get dbSharedOpen => '细节';

  @override
  String dbSharedCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count 共享连接', one: '$count 共享连接');
    return '$_temp0';
  }

  @override
  String shellNewDeviceTitle(String device, String platform) {
    return '签署于$device,$platform的新装置';
  }

  @override
  String shellNewDeviceMessage(String ago) {
    return '在$ago 中签名。如果不是你, 请撤销它并更改您的账户密码。它最初的日子不能撤销您信任的设备。';
  }

  @override
  String get shellNewDeviceItsMe => '是我,我来了';

  @override
  String get shellNewDeviceRevoke => '撤销...';

  @override
  String shellNewDeviceRevoked(String device) {
    return '\" $device \" 被撤销。更改您的账户密码:签名者知道。';
  }

  @override
  String get devicesStatusUnconfirmed => '未确认';

  @override
  String devicesQuarantineBadge(String date) {
    return '新的, 限制到$date';
  }

  @override
  String get errorDeviceInQuarantine => '此设备最近已签名, 尚不能撤销或签署信任的设备。在信任的设备上执行。';

  @override
  String errorDeviceInQuarantineUntil(String date) {
    return '此设备在最近签名, 在$date 之前无法撤销或签名信任的设备。在信任的设备上执行。';
  }

  @override
  String get errorEnvelopeRateLimited => '从新设备打开保险库的尝试太多, 稍后再试。';

  @override
  String errorEnvelopeRateLimitedUntil(String time) {
    return '从新设备打开保险库的尝试太多。之后再试$time 。';
  }

  @override
  String get settingsSignOutEverywhere => '在所有设备上签名...';

  @override
  String get settingsSignOutEverywhereTitle => '在所有设备上签字?';

  @override
  String get settingsSignOutEverywhereMessage => '此账户的每一个会话都会结束, 包括这个。设备会再次使用账户密码签名; 它们的本地数据会保留。';

  @override
  String get settingsSignOutEverywhereConfirm => '到处签名';

  @override
  String get settingsSignOutEverywhereDone => '在所有设备上签名。';

  @override
  String settingsSignOutEverywhereKept(int count, String date) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: ' 此设备是新设备, 因此您$count 信任的设备被保存在$date 中。新设备的会话结束。',
      one: ' 此设备是新设备, 因此您信任的设备会一直被签名( 直到$date ) 。新设备的会话结束。',
    );
    return '$_temp0';
  }

  @override
  String get settingsDialogAccountPasswordChangedNewDevice => '账户密码已更改。此设备是新设备, 因此您信任的设备会继续签名。';

  @override
  String get dockerShowFull => '全部显示';

  @override
  String get dockerSecretsMasked => '密码,令牌和密钥被隐藏.';

  @override
  String dockerJsonRevealHelp(String name) {
    return '输入保险库密码, 显示$name 的全部检查JSON, 包括密码和令牌。复制后自动清除剪贴板。';
  }

  @override
  String dockerComposeRevealHelp(String project) {
    return '编译文件可以包含密码。输入保险库口令以完整显示$project 的文件。';
  }

  @override
  String dockerComposeTampered(String path) {
    return '在保存期间,$path的临时文件被别人替换或更改在主机上,没有写到$path.';
  }

  @override
  String dockerComposeTamperedRestored(String path, String backup) {
    return '$path在保存期间在主机上被别人取代,原版从备份$backup中恢复.';
  }

  @override
  String dockerComposeTamperedNotRestored(String path, String backup) {
    return '$path在保存期间在主机上被别人取代,原版无法放回,从备份中恢复$backup.';
  }

  @override
  String get dockerComposeUnsafeDirTitle => '要保存到共享目录吗 ?';

  @override
  String dockerComposeUnsafeDirHelp(String dirs) {
    return '工程目录可以由其他用户编写:$dirs. 保存可能不安全:他们可以在撰写时交换临时文件或备份.';
  }

  @override
  String get dockerComposeSaveAnyway => '无论如何保存';

  @override
  String get dockerTerminalHint => '航站楼可完全进入集装箱,包括其环境变量。';

  @override
  String get updateWindowsInstallHelp => 'ConsoleCrypt关闭,安装器更新并重新开始. SSH会话关闭;配置图和保险库保存.';

  @override
  String get updatePortableHelp => '此副本ConsoleCrypt 是从一个未打开的 ZIP 存档中运行, 无法更新。打开下载页并获取安装器或新的 ZIP 存档。配置文件和保险库被保存。';

  @override
  String get updateOpenDownloadPage => '打开下载页面';

  @override
  String get updateErrorNetwork => '无法下载更新: 服务器无法访问或连接中断。请检查连接并重试。';

  @override
  String get updateErrorIntegrity => '下载的文件与签名大小或校验和不匹配, 没有安装。重新尝试下载。';

  @override
  String get updateErrorFeed => '无法验证更新信息( 签名、格式或有效期) 。没有安装任何信息。';

  @override
  String get updateErrorRollback => '服务器返回了比此设备已经看到的旧更新信息。它可能已被替换; 没有安装。';

  @override
  String updateErrorLaunch(String code) {
    return '安装器无法启动Windows 错误$code ) 。 Antivals 软件可能已经屏蔽了它。从下载页下载安装器。';
  }

  @override
  String get updateErrorStorage => '无法将更新保存到临时文件夹。释放磁盘空间并重试。';

  @override
  String updateErrorBrowser(String url) {
    return '浏览器无法打开。下载页面 :$url';
  }

  @override
  String get kubeGitOpsReconcileTooltip => 'Flux 立即将所选资源与 Git 进行比较, 并应用更改( 流量调节)';

  @override
  String get kubeGitOpsReconcileWithSourceTooltip => 'Flux首先获取最新来源Git寄存器或图表,然后同步选定的资源(flux调节--with-source).';

  @override
  String get kubeGitOpsSuspendTooltip => '在恢复同步之前, 豪华停止将Git 的更改应用到选定的资源( 暂停 )';

  @override
  String get kubeGitOpsResumeTooltip => 'Flux 再次将Git 的更改应用到所选资源, 并立即同步( 恢复 )';

  @override
  String get kubeGitOpsResetTooltip => '清除失败的 HelmRelease 安装和升级计数, 因此 Flux 重新尝试( flux conflect conferrelease --reset )';

  @override
  String get kubeGitOpsForceTooltip => 'Flux即使没有改变,也执行一次性Helm安装或升级(flux调节舵释放--force)';

  @override
  String get copyWhatDdl => 'DDL 数据';

  @override
  String get copyWhatValue => '价值';

  @override
  String get dbOpenInEditor => '在编辑器中打开';

  @override
  String get dbUnlockProduction => '解锁生产';

  @override
  String get dbUnlockProductionTitle => '为更改解锁生产';

  @override
  String dbUnlockProductionWarning(String name) {
    return '更改将直接转到生产数据库“ $name ” 。每个更改的语句仍然显示以备审查, 并需要保险库口令。当时间耗尽、锁起来、断开或锁住保险库时, 重新写入锁。';
  }

  @override
  String get dbUnlockFor => '允许写入';

  @override
  String dbMinutes(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count分钟', one: '$count分钟');
    return '$_temp0';
  }

  @override
  String dbProdWriteUntil(String time) {
    return '制作到$time';
  }

  @override
  String dbProdWriteLeft(String time) {
    return '$time 左边';
  }

  @override
  String get dbLockProduction => '锁定';

  @override
  String get dbProdWriteLocked => '制作的文字又被锁上了.';

  @override
  String get dbProdRoleReadOnly => '这个连接的数据库角色不能更改数据,所以服务器即使在生产解锁时也拒绝写入.';

  @override
  String get dbHistoryProdWrite => '生产变化';

  @override
  String get dbSetNull => '设定为 NULL';

  @override
  String get dbCellEditHint => '输入保存、 Esc 取消、 Tab 移动到下一个单元格';

  @override
  String get dbPendingEdit => '已更改, 尚未应用';

  @override
  String dbSelectedRows(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '已选中 : $count 行', one: '选中 : $count 行');
    return '$_temp0';
  }

  @override
  String get vaultPassphraseConfirmLabel => '保险库口令';

  @override
  String get vaultPassphraseConfirmWrong => '保险库密码错误。';

  @override
  String dbPreviewTabTitle(String name) {
    return '$name——数据.';
  }

  @override
  String dbDetailsSection(String id) {
    String _temp0 = intl.Intl.selectLogic(id, {
      'general': '常规',
      'size': '大小',
      'structure': '结构',
      'partitioning': '分区',
      'maintenance': '维修',
      'replication': '复制',
      'storage': '存储',
      'other': '$id',
    });
    return '$_temp0';
  }

  @override
  String dbDetailsField(String id) {
    String _temp0 = intl.Intl.selectLogic(id, {
      'kind': '对象类型',
      'persistence': '存储',
      'schema': '司马',
      'owner': '拥有者',
      'tablespace': '表格空间',
      'comment': '注释',
      'rows': ' 行数',
      'live_rows': '活行数',
      'dead_rows': '死线',
      'total_size': ' 总规模',
      'table_size': '表格数据',
      'index_size': '索引',
      'toast_size': '终于',
      'columns': '列数',
      'indexes': '索引',
      'constraints': '制约因素',
      'triggers': '触发器',
      'definition_length': '定义长度,字符',
      'partition_key': '分区密钥',
      'partitions': '分区',
      'partition_of': '分区',
      'last_vacuum': '最后一个 VACUM',
      'last_autovacuum': '最后一个自动真空',
      'last_analyze': '上个安妮兹',
      'last_autoanalyze': '上次自动分析',
      'engine': '发动机',
      'row_format': ' 行格式',
      'collation': '整理',
      'definer': '定义者',
      'updatable': '升职',
      'security': '警卫',
      'average_row': '平均行数',
      'free_space': '空闲空间',
      'auto_increment': '下一个自动配置(_N)',
      'created': '已创建',
      'updated': '更新',
      'uncompressed_size': ' 未压缩',
      'parts': '活动部分',
      'sorting_key': '排序密钥',
      'primary_key': ' 主密钥',
      'sampling_key': '取样密钥',
      'ttl': 'TTL 电话',
      'metadata_modified': '元数据已更改',
      'type': '类型',
      'encoding': '编码',
      'database': '数据库',
      'memory': '内存',
      'keys': '键',
      'length_bytes': '长度, 字节',
      'length_fields': '字段',
      'length_elements': '要件',
      'length_members': '成员',
      'length_entries': '条目',
      'page_size': '页面大小',
      'page_count': ' 页 次',
      'freelist_count': '免费页面',
      'foreign_keys': '外国密钥',
      'strict': '威胁',
      'without_rowid': ' 没有零散',
      'autoincrement': '担保',
      'version': '版本',
      'table': '图表',
      'keyspace': '键位',
      'base_table': '基数表',
      'id': '身份证',
      'cql_partition_key': '分区密钥',
      'clustering_key': '组合键',
      'compaction': '压缩',
      'compression': '压缩',
      'gc_grace_seconds': 'GC恩典,秒数',
      'default_ttl': '默认 TTL, 秒',
      'caching': '缓存',
      'bloom_filter': 'Bloom 过滤器假阳性',
      'speculative_retry': '光谱重试',
      'cql_partitions': '分区',
      'mean_partition_size': '平均分区大小',
      'strategy': '复制战略',
      'replication': '复制品',
      'durable_writes': '持久写作',
      'tables': '表格',
      'views': '实际意见',
      'types': '用户类型',
      'functions': '职能',
      'other': '$id',
    });
    return '$_temp0';
  }

  @override
  String dbDetailsKeyword(String id) {
    String _temp0 = intl.Intl.selectLogic(id, {
      'table': '图表',
      'partitioned_table': '分区表格',
      'view': '视图',
      'materialized_view': '实际视图',
      'foreign_table': '外国表格',
      'unlogged': ' 未登录',
      'temporary': '临时',
      'index': '指数',
      'yes': ' 对',
      'no': ' 没有',
      'keyspace': '键位',
      'other': '$id',
    });
    return '$_temp0';
  }

  @override
  String get dbDetailsNoExpiry => '无过期';

  @override
  String get dbDetailsJson => 'JSON';

  @override
  String get dbDetailsEmpty => '此对象没有要显示的。';

  @override
  String get copyWhatObjectDetails => '对象细节';

  @override
  String get dbWrapLines => '环行';

  @override
  String settingsTerminalFontMissing(String family, String fallback) {
    return '字体“$family”没有安装——使用$fallback。';
  }

  @override
  String get settingsCaptureProtectionLabel => '从屏幕抓取隐藏窗口';

  @override
  String get settingsCaptureProtectionOff => '永远';

  @override
  String get settingsCaptureProtectionSecrets => '虽然秘密被显示';

  @override
  String get settingsCaptureProtectionAlways => '总是';

  @override
  String get settingsCaptureProtectionHelp => '窗口从截图、录音和屏幕共享(Zoom, Teams)中忽略出来。使用“ 永远 ”, 同事在共享屏幕时不会看到ConsoleCrypt 窗口。';

  @override
  String get sftpActiveContentTitle => '此文件可以运行代码';

  @override
  String sftpActiveContentBody(String name) {
    return '“ $name ” 可以运行代码, 它将作为编辑器的文本打开。';
  }

  @override
  String get sftpActiveContentNoRun => 'ConsoleCrypt 从未运行下载的文件。要运行, 请在服务器上使用终端。';

  @override
  String get sftpActiveContentOpenAsText => '作为文本打开';

  @override
  String get sftpNoTextEditorTitle => '未找到文本编辑器';

  @override
  String sftpNoTextEditorBody(String name) {
    return '“ $name ” 可以运行代码, 因此它只作为文本打开, 但找不到文本编辑器。选择一个编辑器打开它。';
  }

  @override
  String get sftpChooseEditor => '选择编辑器...';

  @override
  String get errorNoTextEditor => '没有找到可以安全打开此文件的文本编辑器。选择使用 Open With 的编辑器。';

  @override
  String get dbExportEscapeFormulas => '逃逸公式(用于Excel)';

  @override
  String settingsClearClipboardSeconds(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count秒) (中文(简体) ).', one: '$count 第二次');
    return '$_temp0抄袭秘密后';
  }

  @override
  String settingsClearClipboardMinutes(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count分钟', one: '$count分钟');
    return '$_temp0抄袭秘密后';
  }

  @override
  String settingsClearClipboardHours(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count小时)', one: '$count小时)');
    return '$_temp0抄袭秘密后';
  }

  @override
  String settingsClearClipboardDays(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count天) 日数', one: '$count日) (中文(简体) ).');
    return '$_temp0抄袭秘密后';
  }

  @override
  String sharingUnreadableEntries(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count 共享条目无法读取并被隐藏.',
      one: '$count 共享条目无法读取并被隐藏.',
    );
    return '$_temp0';
  }

  @override
  String get snippetPackPostgresql => 'SQL · PostgreSQL';

  @override
  String get snippetPackPostgresqlDescription => '活动、锁、大小、索引、真空、复制和设置。用于 psql 或数据库工作空间。';

  @override
  String get snippetPackMysql => 'SQL · MySQL / MariaDB';

  @override
  String get snippetPackMysqlDescription => '进程列表、 InnoDB 锁、大小、索引、复制和语句摘要。对于 Mysql 客户端或数据库的工作空间。';

  @override
  String get snippetPackClickhouse => 'SQL • 点击之家';

  @override
  String get snippetPackClickhouseDescription => '运行和慢化查询、部件、合并、突变、复制和磁盘。用于点击用户或数据库工作空间。';

  @override
  String get snippetPackRedis => '红色';

  @override
  String get snippetPackRedisDescription => '内存、客户端、慢日志、密钥空间和复制。用于 redis- cli 或数据库的工作空间。';

  @override
  String get snippetPackMongodb => '蒙哥达';

  @override
  String get snippetPackMongodbDescription => '操作、服务器和收集统计数据、索引、复制件和配置文件。对于mongosh,有些还在数据库工作空间运行。';

  @override
  String get snippetDbPgActivity => '主动查询';

  @override
  String get snippetDbPgActivityNote => '不闲的会话,时间最长的会话.';

  @override
  String get snippetDbPgLongRunning => '长期查询';

  @override
  String get snippetDbPgLongRunningNote => '查询运行时间长于给定秒数.';

  @override
  String get snippetDbPgBlocking => '锁定树';

  @override
  String get snippetDbPgBlockingNote => '屏蔽会话,等待会话,按深度缩排(PG 9.6+).';

  @override
  String get snippetDbPgTableSizes => '最大的表格';

  @override
  String get snippetDbPgTableSizesNote => '总大小有堆积、指数和TOAST;行数是规划员的估计。';

  @override
  String get snippetDbPgDatabaseSizes => '数据库大小';

  @override
  String get snippetDbPgDatabaseSizesNote => '每个接受连接的数据库大小。';

  @override
  String get snippetDbPgIndexUsage => '索引使用量';

  @override
  String get snippetDbPgIndexUsageNote => '使用最少的索引; idx_scan = 0 表示自上次统计重置以来未使用。请检查复制件, 并保留独有的索引。';

  @override
  String get snippetDbPgDeadTuples => '死粪便和自动蒸发';

  @override
  String get snippetDbPgDeadTuplesNote => 'Bloat用最后的真空来估计死粪 并分析时间';

  @override
  String get snippetDbPgCacheHit => '缓存命中比率';

  @override
  String get snippetDbPgCacheHitNote => '每个数据库从共享缓冲器读取的块份额;OLTP上低于99%值得一看.';

  @override
  String get snippetDbPgReplication => '复制状态';

  @override
  String get snippetDbPgReplicationNote => '连接的复制品在字节和时间上都有滞后,在主机上运行(PG 10+).';

  @override
  String get snippetDbPgSlots => '复制槽';

  @override
  String get snippetDbPgSlotsNote => '槽和它们保留的WAL;一个不活跃的槽永远保留WAL(PG 13+).';

  @override
  String get snippetDbPgConnections => '用户和应用程序的连接';

  @override
  String get snippetDbPgConnectionsNote => '按数据库,用户,应用程序和状态分类的客户端连接.';

  @override
  String get snippetDbPgSettingsChanged => '非默认设置';

  @override
  String get snippetDbPgSettingsChangedNote => '设置其来源不是内置默认值。';

  @override
  String get snippetDbPgColumns => '表格栏';

  @override
  String get snippetDbPgColumnsNote => '有类型、默认和注释的列。';

  @override
  String get snippetDbPgForeignKeys => '表格的外国密钥';

  @override
  String get snippetDbPgForeignKeysNote => '桌子的外国钥匙 和那些参考它。';

  @override
  String get snippetDbPgSequences => '接近极限的序列';

  @override
  String get snippetDbPgSequencesNote => '使用范围超过给定百分比的序列;同时检查列类型(PG 10+).';

  @override
  String get snippetDbPgFkWithoutIndex => '无索引的外国密钥';

  @override
  String get snippetDbPgFkWithoutIndexNote => '列不导出任何索引的外国密钥;删除并更新父方扫描子表.';

  @override
  String get snippetDbPgStatements => '顶级查询( pg_ stat_ state_ statements)';

  @override
  String get snippetDbPgStatementsNote =>
      '需要 pg_stat_声明扩展名( shared_ preload_librarys and CREATE EXTENSION); 否则它失败为“ 关系不存在 ” 。列名用于 PG 13+ 。';

  @override
  String get snippetDbPgExplainAnalyze => '解释性分析';

  @override
  String get snippetDbPgExplainAnalyzeNote => '运行查询以测量真实的计划; 在 BEGIN ... ROLLBACK 内部设置数据更改语句。';

  @override
  String get snippetDbPgCancel => '取消查询';

  @override
  String get snippetDbPgCancelNote => '取消后端的当前查询;会话保持连接。';

  @override
  String get snippetDbPgTerminate => '终止会话';

  @override
  String get snippetDbPgTerminateNote => '断开后端并滚动回其开放交易.';

  @override
  String get snippetDbMysqlProcesslist => '主动查询';

  @override
  String get snippetDbMysqlProcesslistNote => '不睡觉的连接 时间最长';

  @override
  String get snippetDbMysqlLongRunning => '长期查询';

  @override
  String get snippetDbMysqlLongRunningNote => '语句运行时间长于给定秒数。';

  @override
  String get snippetDbMysqlLockWaits => 'InnoDB 锁定等待';

  @override
  String get snippetDbMysqlLockWaitsNote => '谁在等待谁,用屏蔽语句,需要sys schemaMySQL)5.7+,MariaDB)10.6+).';

  @override
  String get snippetDbMysqlTransactions => '打开交易';

  @override
  String get snippetDbMysqlTransactionsNote => 'InnoDB交易,最古老的第一,有锁定和修改的行.';

  @override
  String get snippetDbMysqlTableSizes => '最大的表格';

  @override
  String get snippetDbMysqlTableSizesNote => 'MB中的数据,索引和空闲空间; 信息_schema统计是近似和缓存的.';

  @override
  String get snippetDbMysqlDatabaseSizes => '数据库大小';

  @override
  String get snippetDbMysqlDatabaseSizesNote => 'MB中每个计划的总数据和指数大小.';

  @override
  String get snippetDbMysqlIndexUsage => '索引使用量';

  @override
  String get snippetDbMysqlIndexUsageNote => '自服务器启动以来,每个索引都读写( pperfect_schema).';

  @override
  String get snippetDbMysqlUnusedIndexes => '未使用的索引';

  @override
  String get snippetDbMysqlUnusedIndexesNote => '自服务器启动后未读索引。需要 sys schema; 在下降前检查复制件。';

  @override
  String get snippetDbMysqlAutoIncrement => '接近极限的自动调用';

  @override
  String get snippetDbMysqlAutoIncrementNote => '已使用的列类型范围的共享。需要 sys schema。';

  @override
  String get snippetDbMysqlIndexes => '表格索引';

  @override
  String get snippetDbMysqlIndexesNote => '索引列按主要顺序排列。';

  @override
  String get snippetDbMysqlColumns => '表格栏';

  @override
  String get snippetDbMysqlColumnsNote => '有类型、密钥、默认值和注释的列。';

  @override
  String get snippetDbMysqlForeignKeys => '表格的外国密钥';

  @override
  String get snippetDbMysqlForeignKeysNote => '桌子的外国钥匙 和那些参考它。';

  @override
  String get snippetDbMysqlReplicaStatus => '复制状态';

  @override
  String get snippetDbMysqlReplicaStatusNote => 'MySQL 8.0.22+和MariaDB 10.5.1+;在旧服务器上使用SHOW SLAVE Status.';

  @override
  String get snippetDbMysqlVariables => '服务器变量';

  @override
  String get snippetDbMysqlVariablesNote => '匹配类似模式的全球变量。';

  @override
  String get snippetDbMysqlStatus => '密钥状态计数器';

  @override
  String get snippetDbMysqlStatusNote => 'Uptime,连接,慢查询,行锁等待和缓冲池读取.';

  @override
  String get snippetDbMysqlConnections => '用户和主机的连接';

  @override
  String get snippetDbMysqlConnectionsNote => '按用户,客户端主机,计划及命令分类的连接.';

  @override
  String get snippetDbMysqlDigest => '最高查询(报表摘要)';

  @override
  String get snippetDbMysqlDigestNote => '从性能_schema(默认启用于MySQL;默认关闭于MariaDB)起,按总时间进行规范化的报表.';

  @override
  String get snippetDbMysqlInnodbStatus => 'InnoDB 状态';

  @override
  String get snippetDbMysqlInnodbStatusNote => '引擎报告有最新的僵局、积分和缓冲池;需要程序特权。';

  @override
  String get snippetDbMysqlExplainAnalyze => '解释性分析';

  @override
  String get snippetDbMysqlExplainAnalyzeNote => '运行用于测量真实计划MySQL的查询(8.0.18+;在MariaDB中使用ANALYZE SELECT...).';

  @override
  String get snippetDbMysqlKillQuery => '取消查询';

  @override
  String get snippetDbMysqlKillQueryNote => '停止连接的当前语句; 连接保持打开。';

  @override
  String get snippetDbClickhouseProcesses => '运行中的查询';

  @override
  String get snippetDbClickhouseProcessesNote => '初始查询,包括时间、内存和行读。';

  @override
  String get snippetDbClickhouseLongRunning => '长期查询';

  @override
  String get snippetDbClickhouseLongRunningNote => '查询运行时间长于给定秒数.';

  @override
  String get snippetDbClickhouseSlowQueries => '慢查询';

  @override
  String get snippetDbClickhouseSlowQueriesNote => '从 system.query_log 中最慢完成的最后几个小时的查询。';

  @override
  String get snippetDbClickhouseFailedQueries => '查询失败';

  @override
  String get snippetDbClickhouseFailedQueriesNote => '最近的询问,除此之外,都结束了。';

  @override
  String get snippetDbClickhouseQueryPatterns => '最重的查询模式';

  @override
  String get snippetDbClickhouseQueryPatternsNote => '已完成的查询按正常文本、总持续时间分类。';

  @override
  String get snippetDbClickhouseTableSizes => '最大的表格';

  @override
  String get snippetDbClickhouseTableSizesNote => '用压缩比压缩和未压缩的活性部件大小.';

  @override
  String get snippetDbClickhouseDatabaseSizes => '数据库大小';

  @override
  String get snippetDbClickhouseDatabaseSizesNote => '磁盘使用每个数据库的有效部件。';

  @override
  String get snippetDbClickhousePartitions => '表格分区';

  @override
  String get snippetDbClickhousePartitionsNote => '每个分区的部件、行和大小。';

  @override
  String get snippetDbClickhousePartsCount => '包含多个部分的分区';

  @override
  String get snippetDbClickhousePartsCountNote => '太多的活性部件会减慢插入和选择(“太多部分”)。';

  @override
  String get snippetDbClickhouseColumnSizes => '表格的列大小';

  @override
  String get snippetDbClickhouseColumnSizesNote => '每列压缩大小,比例和编码器.';

  @override
  String get snippetDbClickhouseMerges => '运行合并';

  @override
  String get snippetDbClickhouseMergesNote => '合并与变异正在进行中.';

  @override
  String get snippetDbClickhouseMutations => '未完成的变异';

  @override
  String get snippetDbClickhouseMutationsNote => '还有 最新消息/数据还在运行 最新故障原因';

  @override
  String get snippetDbClickhouseReplicationQueue => '复制队列';

  @override
  String get snippetDbClickhouseReplicationQueueNote => '旧的复制任务,有重试和最后一个错误(复制*表).';

  @override
  String get snippetDbClickhouseReplicas => '复制状态';

  @override
  String get snippetDbClickhouseReplicasNote => '延迟、队列和只读状态 重复* 表格。';

  @override
  String get snippetDbClickhouseDisks => '磁盘';

  @override
  String get snippetDbClickhouseDisksNote => '所配置磁盘的空闲和总空间。';

  @override
  String get snippetDbClickhouseErrors => '服务器错误';

  @override
  String get snippetDbClickhouseErrorsNote => '从最新消息开始算出错误。';

  @override
  String get snippetDbClickhouseSettingsChanged => '更改的设置';

  @override
  String get snippetDbClickhouseSettingsChangedNote => '与默认不同的会话设置。';

  @override
  String get snippetDbClickhouseMetrics => '当前衡量标准';

  @override
  String get snippetDbClickhouseMetricsNote => '非零度量衡:查询,合并,连接,内存.';

  @override
  String get snippetDbClickhouseExplain => '带有索引的演示文稿';

  @override
  String get snippetDbClickhouseExplainNote => '有主密钥的计划并跳过使用的索引;查询没有执行。';

  @override
  String get snippetDbClickhouseKillQuery => '取消查询';

  @override
  String get snippetDbClickhouseKillQueryNote => '使用给定的查询_id停止查询。';

  @override
  String get snippetDbRedisInfo => '服务器信息';

  @override
  String get snippetDbRedisInfoNote => '一个INFO部分:服务器,客户端,内存,持久性,统计,复制,cpu,密钥空间.';

  @override
  String get snippetDbRedisMemory => '内存';

  @override
  String get snippetDbRedisMemoryNote => '使用和峰值内存,破碎和最大值政策.';

  @override
  String get snippetDbRedisMemoryDoctor => '记忆医生';

  @override
  String get snippetDbRedisMemoryDoctorNote => 'Redis关于记忆问题的建议.';

  @override
  String get snippetDbRedisSlowlog => '慢记录';

  @override
  String get snippetDbRedisSlowlogNote => '最新慢命令,持续时间为微秒。';

  @override
  String get snippetDbRedisClients => '客户列表';

  @override
  String get snippetDbRedisClientsNote => '连接客户端的地址,年龄,闲暇时间和最后的命令.';

  @override
  String get snippetDbRedisDbsize => '键数';

  @override
  String get snippetDbRedisDbsizeNote => '选定数据库中的密钥。';

  @override
  String get snippetDbRedisKeyspace => '键位';

  @override
  String get snippetDbRedisKeyspaceNote => '每个数据库的密钥和过期密钥。';

  @override
  String get snippetDbRedisScan => '按模式查找密钥';

  @override
  String get snippetDbRedisScanNote => '递增 SCAN 而不是 nubeS; 重复返回的光标直到 0。';

  @override
  String get snippetDbRedisType => '密钥类型';

  @override
  String get snippetDbRedisTypeNote => '存储在密钥上的数据类型。';

  @override
  String get snippetDbRedisTtl => '密钥 TTL';

  @override
  String get snippetDbRedisTtlNote => '秒到过期; -1 表示无过期, -2 表示密钥不存在.';

  @override
  String get snippetDbRedisMemoryUsage => '关键内存使用';

  @override
  String get snippetDbRedisMemoryUsageNote => '键及其值使用的字节。';

  @override
  String get snippetDbRedisLatency => '临时医生';

  @override
  String get snippetDbRedisLatencyNote => '延迟报告;需要延迟-监测-阈值 > 0。';

  @override
  String get snippetDbRedisConfig => '配置值';

  @override
  String get snippetDbRedisConfigNote => '匹配 glob 模式的配置参数。';

  @override
  String get snippetDbRedisReplication => '复制';

  @override
  String get snippetDbRedisReplicationNote => '角色、连接复制品和复制品抵消。';

  @override
  String get snippetDbRedisCommandstats => '命令统计';

  @override
  String get snippetDbRedisCommandstatsNote => '自上次重置以来,每个命令的呼叫和时间。';

  @override
  String get snippetDbMongodbCurrentOps => '现行业务';

  @override
  String get snippetDbMongodbCurrentOpsNote => '业务运行现在(蒙古语).';

  @override
  String get snippetDbMongodbLongOps => '长期运作';

  @override
  String get snippetDbMongodbLongOpsNote => '操作运行时间长于给定秒数( mongosh) 。';

  @override
  String get snippetDbMongodbConnections => '连接';

  @override
  String get snippetDbMongodbConnectionsNote => '目前,可用和总创建的连接(mongosh).';

  @override
  String get snippetDbMongodbOpcounters => '行动柜台';

  @override
  String get snippetDbMongodbOpcountersNote => '开始后插入,查询,更新,删除和命令(mongosh).';

  @override
  String get snippetDbMongodbMemory => '内存';

  @override
  String get snippetDbMongodbMemoryNote => 'MB (mongosh) 中的驻地和虚拟内存.';

  @override
  String get snippetDbMongodbDbStats => '数据库统计数据';

  @override
  String get snippetDbMongodbDbStatsNote => 'MB中的收藏,对象,数据和索引大小.';

  @override
  String get snippetDbMongodbCollectionStats => '收集数据';

  @override
  String get snippetDbMongodbCollectionStatsNote => 'MB中的文档,存储和每个索引大小.';

  @override
  String get snippetDbMongodbIndexes => '收藏索引';

  @override
  String get snippetDbMongodbIndexesNote => '索引键和选项。';

  @override
  String get snippetDbMongodbIndexStats => '索引使用量';

  @override
  String get snippetDbMongodbIndexStatsNote => '自上次重启( mongosh) 以来的每个索引操作。';

  @override
  String get snippetDbMongodbExplain => '统计查询计划';

  @override
  String get snippetDbMongodbExplainNote => '运行发现并报告检查的计划、钥匙和文件(蒙古文)。';

  @override
  String get snippetDbMongodbRsStatus => '复制设定状态';

  @override
  String get snippetDbMongodbRsStatusNote => '成员及其状况和健康(蒙古语)。';

  @override
  String get snippetDbMongodbRsLag => '复制滞后';

  @override
  String get snippetDbMongodbRsLagNote => '每所中学在小学后面有多远(蒙戈什,蒙戈DB 4.4+).';

  @override
  String get snippetDbMongodbProfilerStatus => '配置器级别';

  @override
  String get snippetDbMongodbProfilerStatusNote => '配置器级别和当前数据库的慢化阈值(mongosh).';

  @override
  String get snippetDbMongodbSlowOps => '操作缓慢(显示器)';

  @override
  String get snippetDbMongodbSlowOpsNote => '最新剖面操作比阈值慢; 可用 db. set Profiling Level(1) 。';

  @override
  String get snippetDbMongodbKillOp => '杀死操作';

  @override
  String get snippetDbMongodbKillOpNote => '从当前 Op( mongosh) 中以给定的 opid 停止操作。';

  @override
  String snippetStarterDbOrigin(String note, String client, String package) {
    return '$note用于$client或数据库工作空间. from the $package starter package.';
  }

  @override
  String snippetStarterDbTerminalOrigin(String note, String client, String package) {
    return '$note运行于$client一个终端;数据库工作空间不允许它。从$package启动软件包开始。';
  }

  @override
  String get snippetClientSessionRequired =>
      '在连接的终端标签上先启动数据库客户端(psql, mysql, 点击house-cli, redis-cli, mongosh),然后运行那里片段.';

  @override
  String get dbSnippetPickerTitle => '插入片段';

  @override
  String dbSnippetPickerSearch(String engine) {
    return '搜索$engine 片断';
  }

  @override
  String dbSnippetPickerEmpty(String engine) {
    return '编号$engine 尚有片段。在片段包中添加它们。或保存历史的查询。';
  }

  @override
  String get sharingLeave => '休假份额';

  @override
  String get sharingLeaveHelp => '您停止在您的设备中接收此项目, 它会从列表中消失。您保存的个人副本。主人看到你离开了。';

  @override
  String get sharingLeft => '你离开了股份';

  @override
  String get sharingStopRestoreHelp => '接收者立即失去访问权限。 30天内, 您可以从最近删除的项目中恢复。';

  @override
  String sharingMembersLeft(int count) {
    return '留下的份额:$count';
  }

  @override
  String get sharingRecentlyDeleted => '最近删除';

  @override
  String get sharingRecentlyDeletedHelp => '您删除的共享项目可恢复30天。被恢复的项目将再次与除离开者以外的同一人共享; 他们将再次接受。';

  @override
  String get sharingRecentlyDeletedEmpty => '没有最近删除的项目';

  @override
  String get sharingRestore => '恢复';

  @override
  String get sharingRestored => '恢复和再次共享项目';

  @override
  String sharingRestoreUntil(String date, int count) {
    return '恢复至$date 接受者:$count';
  }

  @override
  String loginSsoButton(String name) {
    return '签入$name';
  }

  @override
  String get loginSsoButtonDefault => '与 SSO 签约';

  @override
  String get loginSsoEmailLabel => '电子邮件( 可选)';

  @override
  String get loginSsoEmailHelper => '预填在您的组织登录页上。';

  @override
  String get loginSsoWaiting => '在浏览器中完成签名...';

  @override
  String get loginSsoWaitingHelp => '浏览器显示您的组织登录页面。当它说到时返回这里。';

  @override
  String get loginLdapLoginLabel => '登录( 电子邮件或 DOMAIN\\user)';

  @override
  String get loginLdapPasswordLabel => '密码';

  @override
  String get loginLdapCaption => '你组织的账户';

  @override
  String get loginLdapLoginRequired => '输入您的登录';

  @override
  String get corporateAuthNoticeTitle => '公司注册';

  @override
  String corporateAuthNotice(String name) {
    return '通过公司授权服务器$name 签名。您组织的密码不会打开保险库 —— 保险库口令。';
  }

  @override
  String get authProviderOidc => '奥伊德科';

  @override
  String get authProviderLdap => '活动目录';

  @override
  String get authProviderSsoFallback => '特别苏';

  @override
  String get serverNeedsNewerApp => '此服务器使用一种签名方式, 此版本不支持。更新ConsoleCrypt。';

  @override
  String get signInMethodEmail => '电子邮件和密码';

  @override
  String signInMethodOidc(String name) {
    return '$name(OIDC)';
  }

  @override
  String signInMethodLdap(String name) {
    return '$name(主动目录)';
  }

  @override
  String signInLastTime(String method, String login) {
    return '你上次和$method签到是$login';
  }

  @override
  String signInLastTimeNoLogin(String method) {
    return '上次你和$method签约';
  }

  @override
  String get signInForget => '忘记';

  @override
  String authModeChangedToEnterprise(String name) {
    return '服务器通过公司授权服务器$name切换到登录. 先前的账户密码不再使用.';
  }

  @override
  String get authModeChangedToStandard => '服务器从公司的登录切换到电子邮件和账户密码。使用您的管理员为您的账户设置的密码。';

  @override
  String get recentServersLabel => '最近的服务器';

  @override
  String get authBadgeEmail => '电子邮件';

  @override
  String authBadgeSso(String name) {
    return '证交所:$name';
  }

  @override
  String get authBadgeAd => '自动';

  @override
  String get settingsAuthLabel => '授权';

  @override
  String settingsAuthCorporate(String name, String kind) {
    return '公司服务器——$name$kind';
  }

  @override
  String get settingsAuthStandard => '电子邮件和账户密码';

  @override
  String get settingsAuthCorporateHelp => '您组织的授权服务器检查谁签名。它永远无法进入保险库: 保险库口令会留在您的设备上。';

  @override
  String settingsAuthSignedIn(String method, String date) {
    return '这个装置与$method在$date上签字';
  }

  @override
  String settingsAuthSignedInAs(String method, String login, String date) {
    return '这个装置与$method在$date上签为$login';
  }

  @override
  String devicesAuthCorporate(String name) {
    return '通过公司授权服务器$name 登录此服务器的账户。';
  }

  @override
  String get accountDisabledTitle => '管理员已禁用访问';

  @override
  String accountDisabledBody(String server) {
    return '您的组织在 $server 上禁用此账户。同步停止, 配置文件被锁定。此设备的数据会保留加密。';
  }

  @override
  String get accountDisabledSignIn => '再次签名';

  @override
  String get accountDisabledDelete => '从此设备中删除配置';

  @override
  String accountDisabledDeleteTitle(String name) {
    return '删除“$name”';
  }

  @override
  String get accountDisabledDeleteMessage => '从此设备中删除加密本地副本。服务器上没有任何变化。';

  @override
  String get errorAccountDisabled => '管理员禁止访问。';

  @override
  String get errorEnterpriseAuthRequired => '此服务器只接受您的组织登录。请检查服务器, 或更新应用程序。';

  @override
  String get errorIdentityConflict => '您的组织的账户已经链接到服务器上的另一个账户。请联系管理员。';

  @override
  String get errorDomainNotAllowed => '您的电子邮件域不允许在此服务器上。';

  @override
  String get errorGroupRequired => '您的账户不在允许使用此服务器的组中。联系管理员。';

  @override
  String get errorMfaRequired => '您的组织需要签入第二个因素。再次签名使用它。';

  @override
  String get errorIdpUnavailable => '你组织的登录服务不可用 稍后再试';

  @override
  String get errorReasonEnterpriseCallbackInvalid => '浏览器返回了一个出乎意料的答案, 再次开始签名。';

  @override
  String get errorReasonEnterpriseSignInTimeout => '签名时间太长了 重新开始';

  @override
  String get errorReasonEnterpriseBrowserUnavailable => '无法打开浏览器签名。';

  @override
  String get teamsTitle => '团队';

  @override
  String get teamsSubtitle => '团队拥有的资源,每个成员都看到这些资源;角色决定谁可以改变这些资源。';

  @override
  String get teamsEmpty => '你还没有加入任何团队。';

  @override
  String get teamsCreate => '创建团队';

  @override
  String get teamsRefresh => '刷新';

  @override
  String get teamsUnnamed => '未命名团队';

  @override
  String get teamsPendingConfirmation => '等待管理员比较代码';

  @override
  String get teamsUnavailable => '现在不行';

  @override
  String teamsRowMeta(String role, int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count成员', one: '1个成员');
    return '$role · $_temp0';
  }

  @override
  String get teamsWarningBadge => '暂停';

  @override
  String get teamsRotationBadge => '旋转密钥';

  @override
  String get teamsOfflineBadge => '离线';

  @override
  String get teamsReadOnly => '只读';

  @override
  String teamsChipSemantics(String name) {
    return '团队$name';
  }

  @override
  String teamsInvitationFrom(String email, String role) {
    return '$email邀请你加入一个团队作为$role.';
  }

  @override
  String teamsInvitationNote(String note) {
    return '注:$note';
  }

  @override
  String get teamsAccept => '接受';

  @override
  String get teamsDecline => '缩写';

  @override
  String get teamsCreateTitle => '新团队';

  @override
  String get teamsCreateBody => '团队商店的名称,颜色和一切都是端到端加密的,服务器只看到谁是成员.';

  @override
  String get teamsNameLabel => '团队名称';

  @override
  String get teamsDescriptionLabel => '描述( 可选)';

  @override
  String get teamsColorLabel => '颜色';

  @override
  String get teamsColorNone => '团队颜色';

  @override
  String get teamsColorCustom => '自定义...';

  @override
  String teamsCreated(String name) {
    return '创建 \" $name \" 小组';
  }

  @override
  String get teamsJoinCodeTitle => '比较代码';

  @override
  String teamsJoinCodeBody(String email) {
    return '将这个代码读到$email (亲自、电话或信使) 。他们在屏幕上看到同样的代码, 并进行确认。';
  }

  @override
  String get teamsJoinCodeWait => '管理员确认后 队伍立即打开你的设备';

  @override
  String teamsJoinRequest(String email, String role) {
    return '$email接受了邀请$role. 比较代码后再添加.';
  }

  @override
  String get teamsCompareCode => '比较代码';

  @override
  String teamsCompareTitle(String email) {
    return '添加$email吗?';
  }

  @override
  String teamsCompareRole(String role) {
    return '作用:$role';
  }

  @override
  String teamsCompareBody(String email) {
    return '要求 $email 在屏幕上读取代码。只有在每个组匹配时才会添加代码。';
  }

  @override
  String teamsCodesMatchCheckbox(String email) {
    return '$email屏幕上的代码是相同的';
  }

  @override
  String get teamsCompareWarning => '不同的代码表示有人可能会拦截邀请。不要添加成员。';

  @override
  String get teamsCodesDontMatch => '密码不匹配';

  @override
  String get teamsAddMember => '添加到团队';

  @override
  String teamsMemberAdded(String email) {
    return '$email加入球队.';
  }

  @override
  String get teamsCodeRejectedTitle => '拒绝加入';

  @override
  String get teamsCodeRejectedBody => '没有人被添加。再次邀请该个人 并比较代码与您信任的频道。';

  @override
  String get teamsTabMembers => '成员';

  @override
  String get teamsTabSettings => '设置';

  @override
  String get teamsTabHistory => '历史';

  @override
  String get teamsWarningInvalidLog => '服务器显示一个成员更改, 没有人签名。团队暂停( 仅读), 并且没有共享密钥, 直到解决。';

  @override
  String get teamsWarningRollback => '服务器显示的团队成员比以前老或不同,团队暂停(只读).';

  @override
  String get teamsRotationRequired => '一个成员离开了团队,旋转键,这样他们就再也看不下去了';

  @override
  String get teamsRotateKey => '旋转密钥';

  @override
  String get teamsKeyRotated => '团队密钥旋转';

  @override
  String teamsMemberYou(String email) {
    return '$email (中文(简体) ).';
  }

  @override
  String get teamsPendingInvitations => '发出的邀请';

  @override
  String get teamsCancelInvitation => '取消';

  @override
  String get teamsInvite => '邀请成员';

  @override
  String get teamsTransferOwner => '让主人';

  @override
  String teamsTransferOwnerConfirm(String email) {
    return '$email成为球队主力;你留守一个管理员.';
  }

  @override
  String get teamsRemoveMember => '从团队中删除';

  @override
  String teamsRemoveMemberConfirm(String email) {
    return '$email 失去进入球队的机会. 球队的密钥被旋转,所以以后不会增加任何内容,对他们来说是可读的.';
  }

  @override
  String teamsInviteTitle(String name) {
    return '邀请参加$name';
  }

  @override
  String get teamsInviteBody => '此人需要在这个服务器上有一个ConsoleCrypt账户。他们在接受后,在他们访问之前,你先与他们比较一个代码。';

  @override
  String get teamsInviteEmail => '电子邮件';

  @override
  String get teamsRoleLabel => '作用';

  @override
  String get teamsInviteNote => '关于邀请的说明(可选)';

  @override
  String get teamsInviteNoteWarning => '注释没有加密:服务器可以读取.';

  @override
  String get teamsInviteSend => '发送邀请';

  @override
  String teamsInvited(String email) {
    return '邀请函$email';
  }

  @override
  String get teamsSettingsReadOnly => '只有所有者和管理员更改团队设置。';

  @override
  String get teamsPoliciesTitle => '政策';

  @override
  String get teamsPolicyProduction => '将新的Kubernetes背景和数据库标为生产';

  @override
  String get teamsPolicyBlockAi => '永远不要将这些资源发送给AI助手';

  @override
  String get teamsPolicyConfirmRisky => '运行危险命令前询问';

  @override
  String get teamsPolicyClipboard => '清除复制后的秘密';

  @override
  String get teamsPolicyClipboardDefault => 'App 默认';

  @override
  String teamsPolicySeconds(int seconds) {
    return '$seconds s (中文(简体) ).';
  }

  @override
  String get teamsSettingsSaved => '团队设置已保存';

  @override
  String get teamsMyColorTitle => '这支队伍的颜色';

  @override
  String get teamsMyColorBody => '只更改设备上的颜色。';

  @override
  String get teamsLeave => '离开小组';

  @override
  String teamsLeaveConfirm(String name) {
    return '你失去了所有设备上的 \" $name \" 。';
  }

  @override
  String teamsKeyEpoch(int epoch) {
    return '钥匙生成$epoch';
  }

  @override
  String teamsLogGenesis(String actor) {
    return '$actor 创建团队';
  }

  @override
  String teamsLogAdd(String actor, String member, String role) {
    return '$actor 增加$member为$role';
  }

  @override
  String teamsLogRole(String actor, String member, String role) {
    return '$actor将$member的角色改为$role.';
  }

  @override
  String teamsLogRemove(String actor, String member) {
    return '$actor 已删除$member';
  }

  @override
  String teamsLogLeave(String actor) {
    return '$actor离开球队';
  }

  @override
  String teamsLogTransfer(String actor, String member) {
    return '$actor 制作$member';
  }

  @override
  String teamsLogRotate(String actor, int epoch) {
    return '$actor旋转键(生成$epoch).';
  }

  @override
  String get teamsRoleOwner => '拥有者';

  @override
  String get teamsRoleAdmin => '管理员';

  @override
  String get teamsRoleEditor => '编辑器';

  @override
  String get teamsRoleViewer => '查看器';

  @override
  String get teamsErrorReadOnly => '你只能查看这个团队的资源.';

  @override
  String get teamsErrorUntrusted => '小组暂停工作:其成员无法核实。';

  @override
  String get teamsErrorCrossVault => '团队资源只能使用同一团队的资源,先复制证书或跳入团队.';

  @override
  String get teamsErrorPersonalOnly => '这东西留在你的私人保险库里';

  @override
  String get teamsErrorAiBlocked => '团队政策不允许将这一资源发送给AI助手.';

  @override
  String get teamsErrorOwnerCannotLeave => '离开前让另一个成员成为主人';

  @override
  String get teamsFilterAll => '全部';

  @override
  String get teamsFilterPersonal => '个人';

  @override
  String get teamsCopyTo => '复制到团队...';

  @override
  String get teamsMoveTo => '移动到团队...';

  @override
  String get teamsMoveToPersonal => '移动到个人';

  @override
  String get teamsPickTitle => '选择团队';

  @override
  String teamsCopied(String name) {
    return '复制至$name';
  }

  @override
  String teamsMoved(String name) {
    return '迁至$name';
  }

  @override
  String get teamsNewGroup => '新团队';

  @override
  String get teamsNewGroupHint => '在团队组中创建的主机属于团队.';

  @override
  String teamsPolicyAck(String host) {
    return '我检查了这个命令$host(团队政策)';
  }

  @override
  String get notesChooseHost => '选择主机';

  @override
  String get notesChooseHostEllipsis => '选择主机...';

  @override
  String get notesChosenClear => '返回此区域的对象';

  @override
  String get notesNoObject => '未选择对象';

  @override
  String get notesNoObjectTitle => '关于主机的说明';

  @override
  String get notesNoObjectMessage => '关于主机的注释在此显示。打开终端标签或选择主机。';

  @override
  String get notesSaved => '已保存';

  @override
  String get notesUnsaved => '未保存的更改';

  @override
  String get notesSaveTooltip => '保存 (QQS / Ctrl+S)';

  @override
  String get notesUnsavedTitle => '不省钱就走?';

  @override
  String get notesUnsavedMessage => '无法保存该注释。您的最新更改将丢失。';

  @override
  String get notesDiscardChanges => '丢弃更改';

  @override
  String hostsNotesCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count 注释', one: '1个注解');
    return '$_temp0';
  }

  @override
  String get notesNotLinked => '未链接';

  @override
  String notesNotLinkedWas(String name) {
    return '未链接( 是 $name )';
  }

  @override
  String get notesAttachTo => '附着于...';

  @override
  String get notesFilter => '显示';

  @override
  String get notesFilterAll => '所有笔记';

  @override
  String notesFilterNotLinked(int count) {
    return '未链接 $count )';
  }

  @override
  String get dbFolderKeys => '键';

  @override
  String get dbFolderForeignKeys => '外国密钥';

  @override
  String get dbFolderReferences => '参考资料';

  @override
  String get dbFolderDataTypes => '数据类型';

  @override
  String get dbFolderTableTriggers => '表格触发器';

  @override
  String get dbLocalOnly => '仅此设备';

  @override
  String get dbLocalOnlyHelp => '仅存储于此设备: 不同步, 备份到保险库或共享, 因为旧的应用版本无法打开此连接类型。';

  @override
  String get dbSqlitePath => '数据库文件路径';

  @override
  String get dbSqliteLocation => '文件位置';

  @override
  String get dbSqliteThisComputer => '这台电脑';

  @override
  String get dbSqliteSshHost => 'SSH主机';

  @override
  String get dbSqliteDocker => 'Docker集装箱';

  @override
  String get dbSqliteKubernetes => 'Kubernetes 波德语';

  @override
  String get dbSqliteKubeContext => 'Kubernetes 语境';

  @override
  String get dbSqliteChooseFile => '选择文件...';

  @override
  String get dbSqliteCreateFile => '创建新数据库...';

  @override
  String get dbSqliteReadOnly => '只读';

  @override
  String get dbSqliteReadOnlyHelp => '文件打开只读:没有查询可以更改它.';

  @override
  String get dbSqliteRemoteHelp => '查询在主机,容器或Pod上运行的sqlite3程序(3.16或较新);文件没有复制.';

  @override
  String get dbSqliteLocalUnavailable => '仅在桌面应用程序中打开此设备上的文件。请选择主机SSH、容器或 Pod。';

  @override
  String get dbErrorSqliteNotDatabase => '文件不是SQLite数据库(或被加密).';

  @override
  String get dbErrorSqliteMissing => '没有找到数据库文件。只创建“ 创建新数据库” 的新文件。';

  @override
  String get dbErrorSqliteCli => 'sqlite3(3.16或较新)没有安装在主机、容器或Pod上。安装后再试一次。';

  @override
  String get dbErrorKubeExecBlocked => '在此生产Kubernetes 上下文中运行的命令会在其设置中关闭。';

  @override
  String get clipboardSecureCopyFailed => '无法安全地复制 剪贴板上什么都没有';

  @override
  String get copiedSecretPrivateNote => '它不会出现在剪贴板历史或其他设备上.';

  @override
  String get settingsClipboardBackgroundClear => '清除背景中的剪贴板, 即使无法校验密钥';

  @override
  String get settingsClipboardBackgroundClearHelp =>
      'Android 不允许背景中的应用程序读取剪贴板。当它打开时, 剪贴板在截止时间被清空, 没有检查, 因此您在此期间复制到另一个应用程序的文本也可能被删除。当它关闭时, 秘诀会留在剪贴板上, 直到返回ConsoleCrypt。';

  @override
  String get settingsClipboardBackgroundClearError => '不能读或保存这个设置。';

  @override
  String get dbNullCell => '[努';

  @override
  String get dbDefaultCell => '[防';

  @override
  String get dbRowAddedHint => '新建行, 尚未保存';

  @override
  String get dbRowDeletedHint => '保存时将被删除';

  @override
  String get dbRequiredValue => '需要一个值';

  @override
  String get dbKeyColumn => '键栏';

  @override
  String get dbTypeNumber => '数目';

  @override
  String get dbTypeText => '文本';

  @override
  String get dbTypeDate => '日期和时间';

  @override
  String get dbTypeBool => '真实/虚假';

  @override
  String get dbTypeJson => 'JSON';

  @override
  String get dbTypeBinary => '二进制';

  @override
  String get dbEditUnsupported => '此处不可用行编辑';

  @override
  String get dbEditEngineUnsupported => '此数据库类型的行无法在此编辑';

  @override
  String get dbEditDesktopOnly => '在计算机上编辑行';

  @override
  String get dbEditNoKey => '编辑需要一个带有主键或唯一键的表格查询';

  @override
  String dbEditPartitions(int count) {
    return '行在 $count 分区中。每个分区会全部更改或无更改, 但不会与其他分区一起更改 : 有些分区可以更改, 而其他分区不能更改。请检查这些报表并输入库口令。';
  }

  @override
  String get dbEditCqlKey => '无法编辑 Cassandra / ScyllaDB 表格的关键列';

  @override
  String get dbEditProductionLocked => '制作只读: 解锁先写';

  @override
  String get dbEditReadOnly => '会话为只读: 允许先写';

  @override
  String dbRequiredMissing(String columns) {
    return '填写所需栏:$columns';
  }

  @override
  String get dbSaveFailed => '未保存更改';

  @override
  String get dbUnsavedTitle => '未保存的更改';

  @override
  String dbUnsavedMessage(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count行有变化,没有保存.', one: '1 行有未保存的更改。');
    return '$_temp0 先救他们?';
  }

  @override
  String get dbSave => '保存';

  @override
  String get dbSaveOptions => '保存选项';

  @override
  String get dbShowSql => '显示SQL';

  @override
  String get dbPendingSql => '以SQL计待修改';

  @override
  String get dbRefreshReadOnly => '只更新读取查询的结果';

  @override
  String get dbRefreshUnavailable => '此结果已无法刷新; 再次运行查询';

  @override
  String get dbAutoRefresh => '自动更新';

  @override
  String get dbAutoRefreshOff => '关';

  @override
  String dbEverySeconds(int seconds) {
    return '每$seconds s (中文(简体) ).';
  }

  @override
  String dbEveryMinutes(int minutes) {
    return '每分钟$minutes分钟)';
  }

  @override
  String get dbEditRow => '编辑当前行';

  @override
  String get dbAddRow => '添加一行';

  @override
  String get dbDuplicateRow => '复制行';

  @override
  String get dbDeleteRow => '删除行';

  @override
  String get dbFirstRow => '第一行 :';

  @override
  String get dbPreviousRow => '上一行';

  @override
  String get dbNextRow => '下一行';

  @override
  String get dbLastRow => '最后一条';

  @override
  String get dbExportData => '导出数据...';

  @override
  String get dbImportData => '导入数据...';

  @override
  String get dbFetchSize => '每个获取的行数';

  @override
  String dbFetchRows(int count) {
    return '$count行数';
  }

  @override
  String get dbFetchSizeInvalid => '输入数字从 1 到 1000';

  @override
  String get dbLoadNext => '装入下一行';

  @override
  String get dbFetchAll => '获取所有行';

  @override
  String dbRowsFetched(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count 行已获取', one: '1 行已获取');
    return '$_temp0';
  }

  @override
  String dbFetchedTiming(String seconds, String date, String time) {
    return '$seconds s, $date at $time (中文(简体) ).';
  }

  @override
  String get dbMoreAvailable => '更多可用数据';

  @override
  String dbPendingChanges(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count 未保存的更改', one: ' 未保存1个更改');
    return '$_temp0';
  }

  @override
  String get dbViewTable => '图表';

  @override
  String get dbViewText => '文本';

  @override
  String get dbViewRecord => '记录';

  @override
  String dbTextMoreRows(int count) {
    return '... $count 更多行( 对所有行使用导出)';
  }

  @override
  String get dbNoRows => '无行';

  @override
  String get dbNewRow => '新行';

  @override
  String dbRecordNumber(int row, int count) {
    return '线条 $row 页:1 $count';
  }

  @override
  String get dbKeepValue => '保留值';

  @override
  String get dbRecordEditHint => '单击以更改值';

  @override
  String get dbUseDefault => '留给服务器( 默认)';

  @override
  String dbSizeMb(String size) {
    return '$size 甲基溴';
  }

  @override
  String dbSizeKb(int size) {
    return '$size KB (英语).';
  }

  @override
  String dbCsvTitle(String table) {
    return '将CSV导入$table';
  }

  @override
  String dbCsvStep(int step, int count, String title) {
    return '步骤$step,$count: $title';
  }

  @override
  String get dbCsvStepFile => '文件';

  @override
  String get dbCsvStepRead => '阅读设置';

  @override
  String get dbCsvStepMap => '列数';

  @override
  String get dbCsvStepOptions => '导入选项';

  @override
  String get dbCsvStepRun => '导入';

  @override
  String dbCsvFileHelp(String size) {
    return '选择 CSV、 TSV 或文本文件, 最多可达 $size 。它被读作此设备上的流。';
  }

  @override
  String get dbCsvChooseFile => '选择文件...';

  @override
  String dbCsvTooLarge(int size) {
    return '文件大于${size}MB,不能在此导入.';
  }

  @override
  String get dbCsvUnreadable => '无法使用这些设置读取文件。';

  @override
  String get dbCsvPrivacy => '文件的内容只转到这个数据库.';

  @override
  String get dbCsvAuto => '检测';

  @override
  String get dbCsvComma => '逗号,';

  @override
  String get dbCsvSemicolon => '分号;';

  @override
  String get dbCsvTab => '制表符';

  @override
  String get dbCsvPipe => '栏';

  @override
  String get dbCsvEncoding => '编码';

  @override
  String get dbCsvEncodingAuto => 'UTF-8(侦查)';

  @override
  String get dbCsvDelimiter => '分隔符';

  @override
  String get dbCsvQuote => '引用字符';

  @override
  String get dbCsvSkipRows => '启动时要跳过的行';

  @override
  String get dbCsvNull => 'NULL 是一个';

  @override
  String get dbCsvNullEmpty => '空字段';

  @override
  String get dbCsvNullNever => '没什么';

  @override
  String get dbCsvDecimal => '小数分隔符';

  @override
  String get dbCsvDecimalDot => '点1.5';

  @override
  String get dbCsvDecimalComma => '逗号 1,5';

  @override
  String get dbCsvDates => '日期';

  @override
  String get dbCsvHeader => '第一行有列名称';

  @override
  String dbCsvDetected(String delimiter, String encoding) {
    return '改为:划界人$delimiter,$encoding';
  }

  @override
  String get dbCsvMapHelp => '每个文件列都跳到表格列或跳过。前1000行对照列类型进行检查。';

  @override
  String get dbCsvSkipColumn => '跳过';

  @override
  String get dbCsvDuplicateTarget => '两个文件栏到同一个表格栏。';

  @override
  String dbCsvMissingRequired(String columns) {
    return '所需列无值:$columns';
  }

  @override
  String dbCsvBadValues(int count, String samples) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count 数值不符合: $samples',
      one: '1 个值不符合:$samples',
    );
    return '$_temp0';
  }

  @override
  String get dbCsvMode => '行数';

  @override
  String get dbCsvModeInsert => '插入';

  @override
  String get dbCsvModeSkip => '插入, 跳过已有密钥';

  @override
  String get dbCsvModeUpsert => '按键插入或更新';

  @override
  String get dbCsvNoKeyClickhouse => '点击之家排序键不独有:行只能添加.';

  @override
  String get dbCsvNoKeyMongo => '将一列映射到_id以跳过或替换已有文档。';

  @override
  String get dbCsvTruncateDocuments => '先删除收藏的每个文档';

  @override
  String get dbCsvMapHelpMongo => '每个文件列都转到一个字段或被跳过;一个列也可以创建其名称的新字段,按每个值(数字、真/假、日期或文本)键入。检查了前1000行。';

  @override
  String get dbCsvMapHelpRedis => '将一列映射到密钥( 文件夹的前缀放在前面 ) 。将一列映射到字符串的值, 或者将几个字段显示其名称( 空的字段被忽略 ) 。';

  @override
  String dbCsvNewField(String name) {
    return '$name——新领域.';
  }

  @override
  String get dbCsvNoKey => '表格没有密钥:行只能插入.';

  @override
  String get dbCsvBatch => '每批(1-5000)行数';

  @override
  String get dbCsvBatchInvalid => '输入批量大小从 1 到 5000';

  @override
  String get dbCsvOnError => '排在坏的一排上';

  @override
  String get dbCsvStop => '停止导入';

  @override
  String get dbCsvSkipBad => '跳过并写入错误报告';

  @override
  String get dbCsvTransaction => '交易';

  @override
  String get dbCsvWhole => '整个文件一个( 全部或无)';

  @override
  String get dbCsvPerBatch => '每批一次';

  @override
  String get dbCsvTruncate => '首先删除表格的所有行';

  @override
  String get dbCsvTruncateHelp => '在导入交易中运行 DELETE; 审查显示。';

  @override
  String get dbCsvNoTransaction => '每一个语句都在这里单独承诺( CQL, 或 sqlite3 on a host) : 已停止的导入会保留已写入的行。';

  @override
  String get dbCsvTruncateHelpNoTransaction => '在第一行之前运行, 如果导入停止, 则不会被撤销; 审查显示。';

  @override
  String get dbCsvImport => '审查和导入...';

  @override
  String dbCsvCounts(int read, int imported, int skipped, int failed) {
    return '改为:$read • 导入:$imported * 跳过:$skipped 失败:$failed';
  }

  @override
  String dbCsvRate(int rows) {
    return '$rows行数/秒';
  }

  @override
  String dbCsvRateEta(int rows, String left) {
    return '$rows行/s 大约$left左';
  }

  @override
  String dbCsvDone(int imported, int skipped, int failed) {
    return '完成: $imported 进口, $skipped 跳过, $failed 有错误。';
  }

  @override
  String dbCsvCancelled(int imported) {
    return '已取消。行保留:$imported(只承诺批次)。';
  }

  @override
  String get dbCsvFailed => '导入失败; 打开的交易被卷回。';

  @override
  String dbCsvRowType(int line) {
    return '行$line : 一个值与其列不符。没有保留任何开放式交易。';
  }

  @override
  String dbCsvRowRequired(int line) {
    return '行$line: 需要的列为空。打开的交易没有保存。';
  }

  @override
  String dbCsvRowUnreadable(int line) {
    return '无法读取行$line (编码或大小) 。没有保留任何公开交易。';
  }

  @override
  String get dbCsvReport => '错误报告';

  @override
  String get dbCsvShowReport => '在文件夹中显示';

  @override
  String get dbCsvCreateTable => '从 CSV 创建表格...';

  @override
  String get dbCsvNewTableName => '新建表格名称';

  @override
  String get dbImportCsv => '从 CSV 导入...';

  @override
  String get dbDesignerCreateTable => '创建表格...';

  @override
  String get dbDesignerCreateCollection => '创建收藏...';

  @override
  String get dbDesignerAlterTable => '编辑结构...';

  @override
  String dbDesignerTabTitle(String name) {
    return '设计:$name';
  }

  @override
  String get dbDesignerNewTab => '新建表格';

  @override
  String dbDesignerUnsavedTitle(String name) {
    return '保存更改为$name ?';
  }

  @override
  String get dbDesignerUnsavedMessage => '表格设计师有尚未应用于数据库的更改。';

  @override
  String get dbDesignerDontSave => '别省钱了';

  @override
  String get dbDesignerSaveButton => '保存...';

  @override
  String get dbDesignerLoadFailed => '无法装入表格结构。';

  @override
  String dbDesignerSaved(String name) {
    return '$name:适用的改动';
  }

  @override
  String get dbDesignerUnsavedMark => '未保存的更改';

  @override
  String get dbDesignerDraftNew => '新建, 尚未保存';

  @override
  String get dbDesignerDraftChanged => '已更改, 尚未保存';

  @override
  String dbDesignerSaveTitle(String name) {
    return '$name——保存更改.';
  }

  @override
  String get dbDesignerSqlPreview => 'SQL预览';

  @override
  String get dbDesignerTransactional => '在一个交易中运行:所有语句都适用或没有.';

  @override
  String get dbDesignerNotTransactional => 'DDL 这里不是交易: 如果语句失败, 则该语句之前的语句继续应用。';

  @override
  String get dbDesignerDestructive => '脚本会丢弃数据或重写表格(丢弃的列,类型或密钥更改).';

  @override
  String get dbDesignerProductionLocked => '这是一个生产连接。解锁生产在工作空间头里写来运行脚本。';

  @override
  String get dbDesignerReadOnly => '会话是只读的。允许在工作空间标题中写入来运行脚本。';

  @override
  String get dbDesignerConfirm => '输入库口令以运行脚本。';

  @override
  String get dbDesignerFailedRolledBack => '声明失败; 没有任何改变。';

  @override
  String get dbDesignerFailedPartly => '一份声明失败;它面前的声明被应用。';

  @override
  String dbDesignerStatementResult(int index, String status) {
    return '$index. $status';
  }

  @override
  String get dbDesignerStatusDone => '应用';

  @override
  String get dbDesignerStatusRolledBack => '回滚';

  @override
  String get dbDesignerStatusSkipped => '未运行';

  @override
  String get dbDesignerStatementFailed => '在服务器上失败';

  @override
  String get dbDesignFields => '字段';

  @override
  String get dbDesignColumns => '列数';

  @override
  String get dbDesignKeys => '键';

  @override
  String get dbDesignForeignKeys => '外国密钥';

  @override
  String get dbDesignIndexes => '索引';

  @override
  String get dbDesignChecks => '检查';

  @override
  String get dbDesignOptions => '选项';

  @override
  String get dbDesignDdl => 'DDL 数据';

  @override
  String get dbDesignHasErrors => '有错误';

  @override
  String get dbDesignNew => '新设';

  @override
  String get dbDesignChanged => '已更改';

  @override
  String get dbDesignPickColumns => '选择列';

  @override
  String get dbDesignCollectionName => '收藏名称';

  @override
  String get dbDesignTableName => '表格名称';

  @override
  String get dbDesignSchema => '司马';

  @override
  String get dbDesignDatabase => '数据库';

  @override
  String get dbDesignComment => '注释';

  @override
  String get dbDesignDescription => '说明';

  @override
  String get dbDesignModeAlter => '编辑结构';

  @override
  String get dbDesignModeCreate => '新建表格';

  @override
  String get dbDesignPending => '已更改';

  @override
  String get dbDesignUndo => '撤销';

  @override
  String get dbDesignRevert => '还原所有更改';

  @override
  String get dbDesignSave => '保存更改';

  @override
  String get dbDesignSaveShortcut => '保存更改 (QQS / Ctrl+S)';

  @override
  String get dbDesignColName => '名称';

  @override
  String get dbDesignColType => '类型';

  @override
  String get dbDesignBsonType => 'BSON 类型';

  @override
  String get dbDesignColLength => '长度';

  @override
  String get dbDesignColScale => '缩放';

  @override
  String get dbDesignRequired => '要求数';

  @override
  String get dbDesignNullable => '可忽略';

  @override
  String get dbDesignNotNull => '不为空';

  @override
  String get dbDesignColDefault => '默认';

  @override
  String get dbDesignColIdentity => '自动递增';

  @override
  String get dbDesignPk => 'PK 键';

  @override
  String get dbDesignUnique => '独一无二';

  @override
  String get dbDesignUnsigned => '未签名';

  @override
  String get dbDesignArray => '阵列';

  @override
  String get dbDesignLowCardinality => '低卡车.';

  @override
  String get dbDesignPickType => '选择类型';

  @override
  String get dbDesignDefaultPresets => '默认预设';

  @override
  String get dbDesignNoDefault => '无默认';

  @override
  String get dbDesignDuplicate => '复制';

  @override
  String get dbDesignMoveUp => '向上移动';

  @override
  String get dbDesignMoveDown => '向下移动';

  @override
  String get dbDesignRemove => '移除';

  @override
  String get dbDesignColumnActions => '列动作';

  @override
  String get dbDesignWillDrop => '将被丢弃';

  @override
  String get dbDesignRestore => '恢复';

  @override
  String get dbDesignAddField => '添加字段';

  @override
  String get dbDesignAddColumn => '添加栏';

  @override
  String get dbDesignPrimaryKey => '主密钥';

  @override
  String get dbDesignUniqueKeys => '独特的制约因素';

  @override
  String get dbDesignAutoName => '名称( 自动)';

  @override
  String get dbDesignAddUnique => '添加独有约束';

  @override
  String get dbDesignRefTable => '参考表格';

  @override
  String get dbDesignPickTable => '选择表格';

  @override
  String get dbDesignOnDelete => '删除时';

  @override
  String get dbDesignOnUpdate => '关于更新';

  @override
  String get dbDesignDefault => '默认';

  @override
  String get dbDesignAddForeignKey => '添加外国密钥';

  @override
  String get dbDesignAddIndex => '添加索引';

  @override
  String get dbDesignCheckExpression => '条件,例如价格. 0';

  @override
  String get dbDesignAddCheck => '添加检查';

  @override
  String get dbDesignCreateOnly => '设定表格创建时';

  @override
  String get dbDesignClickhouseHint => '排序密钥:列名称或表达式,逗号分隔;空使用主密钥. TTL和SETTINGS可以在以后的编辑器中添加.';

  @override
  String get dbDesignFixErrors => '修复错误以查看 DDL。';

  @override
  String get dbDesignNoChanges => '无变动。';

  @override
  String get dbDesignNone => '—';

  @override
  String get dbDesignIdentityByDefault => '身份( 默认)';

  @override
  String get dbDesignIdentityAlways => '身份( 总是)';

  @override
  String get dbDesignIdentitySerial => '序列';

  @override
  String get dbDesignIdentityAutoIncrement => '自动调用(_R)';

  @override
  String get dbDesignOptEngine => '发动机';

  @override
  String get dbDesignOptEngineArgs => '引擎参数';

  @override
  String get dbDesignOptOrderBy => '(按键排序)';

  @override
  String get dbDesignOptPartitionBy => '分区由';

  @override
  String get dbDesignOptPartitionKey => '分区密钥';

  @override
  String get dbDesignOptCharset => '字符集 :';

  @override
  String get dbDesignOptCollation => '整理';

  @override
  String get dbDesignOptAutoIncrement => '自动启动(_N)';

  @override
  String get dbDesignOptCapped => '盖上';

  @override
  String get dbDesignOptSize => '大小, 字节';

  @override
  String get dbDesignOptMax => '最大文档';

  @override
  String get dbDesignOptTimeField => '时间领域(时间序列)';

  @override
  String get dbDesignOptMetaField => '元字段';

  @override
  String get dbDesignOptGranularity => '颗粒度';

  @override
  String get dbDesignOptExpire => '秒后过期';

  @override
  String get dbDesignOptValidationLevel => '验证级别';

  @override
  String get dbDesignOptValidationAction => '审定行动';

  @override
  String get dbDesignOptCollationLocale => '拼接区域';

  @override
  String get dbDesignOptCollationStrength => '整理强度';

  @override
  String get dbDesignIssueNameRequired => '需要一个名字。';

  @override
  String dbDesignIssueNameInvalid(String name) {
    return '“$name......”不能作为这里的名称。';
  }

  @override
  String dbDesignIssueNameTooLong(String name) {
    return '“$name”对于这个数据库来说太长了。';
  }

  @override
  String dbDesignIssueNameSpaces(String name) {
    return '“$name......”从一个空间开始或结束。';
  }

  @override
  String dbDesignIssueNameDuplicate(String name) {
    return '“$name”一词曾两次使用。';
  }

  @override
  String get dbDesignIssueColumnsRequired => '添加至少一个列。';

  @override
  String dbDesignIssueTypeInvalid(String name) {
    return '“$name”不是一个有效的类型。';
  }

  @override
  String dbDesignIssueLengthRequired(String name) {
    return '$name:该型号需要长度.';
  }

  @override
  String dbDesignIssueLengthIgnored(String name) {
    return '$name :该型号没有长度;被忽略.';
  }

  @override
  String dbDesignIssueLengthInvalid(String name) {
    return '$name:检查长度和尺度.';
  }

  @override
  String dbDesignIssueExpressionInvalid(String name) {
    return '“$name”:不是单一的表述(无“;”、评论或不平衡的括号/引文)。';
  }

  @override
  String get dbDesignIssueExpressionRequired => '输入检查条件。';

  @override
  String dbDesignIssueIdentityType(String name) {
    return '$name :自动增量需要整数类型.';
  }

  @override
  String dbDesignIssueIdentityDefault(String name) {
    return '$name:自动递增列不能有默认.';
  }

  @override
  String dbDesignIssueIdentityChange(String name) {
    return '$name : 此自动递增更改不能在此进行; 使用编辑器.';
  }

  @override
  String get dbDesignIssueKeyColumns => '选择至少一个列。';

  @override
  String dbDesignIssueColumnUnknown(String name) {
    return '没有“$name......”栏。';
  }

  @override
  String dbDesignIssueColumnRepeated(String name) {
    return '“$name”栏列出两次。';
  }

  @override
  String get dbDesignIssueRefTable => '选择引用的表格。';

  @override
  String get dbDesignIssueRefColumns => '选择像密钥列一样的引用列。';

  @override
  String dbDesignIssueAutoIncrementKey(String name) {
    return '$name: AUTO_INCREMENT需要一个从本列开始的密钥.';
  }

  @override
  String get dbDesignIssueAutoIncrementSingle => '只有一栏可以是AUTO_INCREMENT.';

  @override
  String dbDesignIssueUnsigned(String name) {
    return '$name:UNSIGNED仅适用于数字类型.';
  }

  @override
  String dbDesignIssueForeignKeysEngine(String name) {
    return '外国密钥需要InnoDB引擎(现在$name).';
  }

  @override
  String get dbDesignIssueOrderBy => '合并Tree表格需要排序密钥:设置 ORDER BY 或主密钥.';

  @override
  String dbDesignIssuePrimaryKeyPrefix(String name) {
    return '主要钥匙$name必须是命令BY的开始.';
  }

  @override
  String dbDesignIssueNullableKey(String name) {
    return '$name:键列不能为Nullable.';
  }

  @override
  String dbDesignIssueEngineOption(String name) {
    return '$name引擎没有键或分区.';
  }

  @override
  String get dbDesignIssuePartitionKey => '输入分区键。';

  @override
  String get dbDesignIssuePartitionPrimaryKey => '分区表的主键必须包括分区键.';

  @override
  String get dbDesignIssueCommentBackslash => '在MySQL和MariaDB中,评论尚不能包含反弹(\\).';

  @override
  String dbDesignIssueOptionInvalid(String name) {
    return '检查数值“$name”。';
  }

  @override
  String dbDesignIssueOptionCreateOnly(String name) {
    return '备选案文“$name”是在创建表格时设定的,在此不作修改。';
  }

  @override
  String get dbDesignIssueCommentChange => '表格注释在此不更改;使用编辑器.';

  @override
  String dbDesignIssueNotNullDefault(String name) {
    return '$name: 如果表格有行,则没有默认的 NULL 不失败。';
  }

  @override
  String dbDesignIssueColumnOrder(String name) {
    return '$name表末增加.';
  }

  @override
  String get dbDesignIssueRename => '这里不支持重新命名或移动表格。';

  @override
  String get dbDesignIssuePrimaryKeyChange => '本表的主键无法更改。';

  @override
  String get dbDesignIssueCappedSize => '封顶收藏需要大小。';

  @override
  String get dbDesignIssueCappedTimeSeries => '时间序列收藏不能封顶。';

  @override
  String get dbDesignIssueTimeField => '先设时场.';

  @override
  String get dbDesignIssueCollationLocale => '先设定整理区域。';

  @override
  String dbDesignIssueDefaultIgnored(String name) {
    return '$name:默认不是集合的一部分,被忽略.';
  }

  @override
  String dbDesignIssueNullableTwice(String name) {
    return '$name:在类型中使用Nullable复选框代替Nullable(.).';
  }

  @override
  String dbDesignIssueOther(String name) {
    return '请检查access-date=中的日期值 (帮助) 设计 $name.';
  }

  @override
  String dbInsertSnippetHelp(String shortcut) {
    return '片段:插入编辑器$shortcut.';
  }

  @override
  String get stepUpOsReason => '确认受保护的行动';

  @override
  String stepUpOsReasonFor(String action) {
    return '确认“$action”';
  }

  @override
  String dbUnlockProductionOsReason(String connection) {
    return '允许在$connection上更改';
  }

  @override
  String get stepUpConfirmTouchId => '用触摸ID确认';

  @override
  String get stepUpConfirmFaceId => '用面部标识确认';

  @override
  String get stepUpConfirmWindowsHello => '以Windows确认';

  @override
  String get stepUpConfirmFingerprint => '指纹确认';

  @override
  String get stepUpConfirmDeviceCredential => '用设备密码确认';

  @override
  String get stepUpUsePassphrase => '输入保险库口令';

  @override
  String stepUpPresenceHint(String method) {
    return '你会用$method确认';
  }

  @override
  String get stepUpPresenceFailed => '未确认。请输入保险库口令。';

  @override
  String get errorPassphraseRequired => '用保险库密码确认';

  @override
  String get confirmActionsWithPresence => '确认使用生物鉴别技术的行动';

  @override
  String confirmActionsWithPresenceHelp(String method) {
    return '$method 而不是用于此设备上风险动作的保险库口令。更改口令、新的恢复工具箱、关闭设备解锁、备份、批准设备和账户安全总是要口令。';
  }

  @override
  String get dbKeyspace => '键位';

  @override
  String get dbCassandraContactPoints => '更多的联系人(主机、逗号分隔)';

  @override
  String get dbCassandraLocalDc => '本地数据中心';

  @override
  String get dbCassandraReadConsistency => '读取一致性';

  @override
  String get dbCassandraWriteConsistency => '写入一致性';

  @override
  String get dbCassandraPageSize => '每页行数';

  @override
  String get dbCassandraRouteHelp =>
      '直接地,请求分布在第一个联系点的数据中心的每一个节点上,当一个联系点失败时移动到其他节点。通过SSH,一个跳跃剖面,一个代理,一个隧道,Kubernetes或Docker,会话通过一个节点工作:第一个回答的节点。有了本地的数据中心,联系点必须在里面。';

  @override
  String get dbErrorDatacenter => '解答的节点位于另一个数据中心而不是配置的本地中心。请检查联系点或本地数据中心。';

  @override
  String get dbErrorEngineChange => '此连接不能切换到该引擎: 此设备上同步连接和保存的连接会被单独存储。反之, 创建新连接。';

  @override
  String get dbErrorDeviceOnly => '此连接只保存在此设备上, 无法共享或同步。';

  @override
  String get dbFolderTypes => '类型';

  @override
  String get dbScanWarningTitle => '全表改为';

  @override
  String get dbScanAllowFiltering => 'ALLOW FILTERING 使集群读取并过滤整个表格的行, 它可以缓慢并加载每个节点。无论如何运行它吗 ?';

  @override
  String get dbScanFullTable => '此查询没有“ 在哪里” 和“ LIMIT” : 它逐页读取整个表格页。无论如何运行它吗 ?';

  @override
  String get snippetPackCassandra => '卡桑德拉/锡拉DB';

  @override
  String get snippetPackCassandraDescription => 'Schema),集群节点,大小估计,分区,TTL和CQL中的角色. For cqlsh或数据库工作空间.';

  @override
  String get snippetDbCqlKeyspaces => '键位和推广';

  @override
  String get snippetDbCqlKeyspacesNote => '每个键盘空间都有复制策略和持久写作.';

  @override
  String get snippetDbCqlTables => '键位表';

  @override
  String get snippetDbCqlTablesNote => '带有注释的表格,默认 TTL 和 gc_ grace_ seconds。';

  @override
  String get snippetDbCqlColumns => '列和主键';

  @override
  String get snippetDbCqlColumnsNote => '表格的列:分区键、按顺序排列的列、静态列和正态列。';

  @override
  String get snippetDbCqlIndexes => '二级指数';

  @override
  String get snippetDbCqlIndexesNote => '带有目标的关键空间的二级索引。';

  @override
  String get snippetDbCqlViews => '实际意见';

  @override
  String get snippetDbCqlViewsNote => '以底表和过滤器实现密钥空间视图。';

  @override
  String get snippetDbCqlTypes => '用户定义类型';

  @override
  String get snippetDbCqlTypesNote => '用户定义的密钥空间类型及其字段。';

  @override
  String get snippetDbCqlLocalNode => '这个节点';

  @override
  String get snippetDbCqlLocalNodeNote => '集群名称,数据中心,架和版本的协调员.';

  @override
  String get snippetDbCqlPeers => '分组同行';

  @override
  String get snippetDbCqlPeersNote => '协调员认为的其他节点:地址,数据中心,架子,版本.';

  @override
  String get snippetDbCqlSizeEstimates => '表规模估计数';

  @override
  String get snippetDbCqlSizeEstimatesNote => '协调员每个符号范围的估计分区和平均分区大小。';

  @override
  String get snippetDbCqlPartition => '一个分区的行';

  @override
  String get snippetDbCqlPartitionNote => '用文本分区键读取一个分区——以CQL读取的有效方式.';

  @override
  String get snippetDbCqlTtl => 'TTL 和列的写入时间';

  @override
  String get snippetDbCqlTtlNote => '在一个分区内保留 TTL 并写入列的时间戳。';

  @override
  String get snippetDbCqlCountPartition => '分区中的行';

  @override
  String get snippetDbCqlCountPartitionNote => '计算一个分区的行(计算整个表格扫描集群)。';

  @override
  String get snippetDbCqlTokens => '分区符号';

  @override
  String get snippetDbCqlTokensNote => '第一个带符号的分区键,看数据如何在环上传播.';

  @override
  String get snippetDbCqlRoles => '角色';

  @override
  String get snippetDbCqlRolesNote => '角色与他们的登录和超级用户旗。';

  @override
  String get snippetDbCqlPermissions => '角色的权限';

  @override
  String get snippetDbCqlPermissionsNote => '任何直接或通过其他角色给予角色的许可.';

  @override
  String get snippetDbCqlCreateTable => '创建时间序列表';

  @override
  String get snippetDbCqlCreateTableNote => '每键分区,最新行先行(分组顺序),行在默认 TTL 后过期.';

  @override
  String get snippetDbCqlInsertJson => '从JSON插入一行,带TTL';

  @override
  String get snippetDbCqlInsertJsonNote => '内衣... JSON 地图 JSON 字段到任何类型的列;行在 TTL 之后过期。';

  @override
  String syncStateThrottled(String time) {
    return '服务器临时限制同步; 恢复于$time';
  }

  @override
  String get syncScreenStateThrottled => '由服务器暂停';

  @override
  String get syncScreenThrottledBody => '服务器从此账户收到过多请求。更改会停留在这个设备上, 并自动发送—— 没有丢失。';

  @override
  String get syncScreenRejectedTitle => '服务器不接受的更改';

  @override
  String syncScreenRejectedBody(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count 更改未被服务器接受, 它们被保存在这个设备上; 问题解决后选择重试。',
      one: '1 更改未被服务器接受。它被保存在这个设备上; 问题解决后选择重试。',
    );
    return '$_temp0';
  }

  @override
  String errorObjectLimit(String limit) {
    return '库中的对象过多( 限制$limit ) 。删除您不再需要的项目, 然后选择重试。';
  }

  @override
  String errorVaultQuota(String size) {
    return '保险库已使用其服务器存储( 限制$size ) 。删除您不再需要的项目, 然后选择重试。';
  }

  @override
  String errorDeviceLimit(String limit) {
    return '达到设备限制$limit——取消你不再使用的设备,然后再次签名.';
  }

  @override
  String errorDeviceDailyLimit(String time) {
    return '今天有太多新设备了,再试一次$time';
  }

  @override
  String get errorVaultKeyMismatch => '服务器返回了不同的保险库密钥。应用未使用此密钥打开任何数据，并已停止同步。';

  @override
  String get errorRecipientUnverified => '首先验证此设备的密钥: 只有经过验证的收件人才能获得这种访问.';

  @override
  String get errorRecipientKeysChanged => '此设备的密钥已更改。在允许访问前再次验证。';

  @override
  String get errorPeerVerificationFailed => '密码不匹配。密钥可能已被替换。不要让访问和联系同事。';

  @override
  String get vaultKeyMismatchTitle => '保险库密钥不匹配';

  @override
  String get vaultKeyMismatchMessage => '服务器发送了不同的保险库密钥。这可能意味着服务器被篡改了。此设备的数据没有改变, 同步停止。请不要输入此设备上的秘密, 并告诉服务器管理员。';

  @override
  String get vaultKeyMismatchLock => '锁定';

  @override
  String get vaultKeyMismatchCopy => '复制诊断';

  @override
  String get vaultKeyMismatchMore => '更多';

  @override
  String get vaultKeyMismatchDetails =>
      '此设备会记住它的保险库密钥的指纹, 并对照它检查它收到的每个密钥。不同的密钥永远不会被接受, 并且没有被重新加密。唯一的出路就是在服务器被检查后重置此设备上的配置文件( 删除并再次连接) 。';

  @override
  String approvalWaitPairingTitle(String device) {
    return '将代码与“$device”进行比较';
  }

  @override
  String approvalWaitPairingHelp(String device) {
    return '“$device”正在批准这一设备,两个屏幕必须显示完全相同的代码——这是从两个设备的钥匙计算出来的。';
  }

  @override
  String approvalWaitPairingCheckbox(String device) {
    return '编码与“$device”上的编码相符。';
  }

  @override
  String get approvalWaitPairingConfirm => '代码匹配';

  @override
  String get approvalWaitPairingConfirmed => '确认 等待批准结束...';

  @override
  String get approveDeviceCodeV2Hint => '此代码取决于两个设备的密钥; 新设备打开此请求后立即显示。';

  @override
  String get devicesVaultKeyUnconfirmedTitle => '默认密钥未确认';

  @override
  String get devicesVaultKeyUnconfirmedMessage => '此设备在没有独立检查( 老服务器或批准设备) 的情况下接收了保险库密钥。请输入一次保险库密码以确认它。';

  @override
  String get devicesVaultKeyConfirm => '用口令确认';

  @override
  String get vaultKeyConfirmTitle => '确认保险库钥匙';

  @override
  String get vaultKeyConfirmed => '默认密钥确认';

  @override
  String get devicesVaultKeyConflictTitle => '设备收到不同的保险库密钥';

  @override
  String devicesVaultKeyConflictMessage(String device) {
    return '“$device“ ” 报告了不同的保险库密钥。控制服务器的人可能已经给了它自己的密钥。此设备保留了它的密钥。 Revoke ”$device 除非你知道为什么。';
  }

  @override
  String get devicesRevokeConflicting => '撤销';

  @override
  String get sharingTrustChosen => '选择( 未核实)';

  @override
  String sharingTrustVerified(String date) {
    return '核实$date';
  }

  @override
  String get sharingTrustVerifiedNoDate => '已验证';

  @override
  String get sharingTrustKeysChanged => '键已更改';

  @override
  String get sharingTrustNew => '尚未选中';

  @override
  String get sharingVerifyAction => '校验';

  @override
  String get sharingGiveAccess => '给予此设备访问权限';

  @override
  String sharingKeysChangedWarning(String device, String email) {
    return '$email设备的密钥$device已经更改。在再次验证之前没有新的访问权限;对新密钥没有重新加密现有股份。';
  }

  @override
  String get sharingVerifiedOnlyHint => '只有经过核实的收件人才能接收秘密,Kubernetes上下文和数据库连接.';

  @override
  String get sharingFingerprint => '密钥指纹';

  @override
  String get peerVerifyTitle => '检查同事的装置';

  @override
  String peerVerifyDevice(String device) {
    return '设备$device';
  }

  @override
  String get peerVerifySafetyNumber => '安全编号';

  @override
  String get peerVerifyQr => 'QR代码';

  @override
  String get peerVerifySafetyNumberHelp =>
      '请您的同事打开您设备的同一支票, 并读出它们的号码—— 无论是通过电话还是亲自, 而不是通过ConsoleCrypt。输入下面: 它必须完全匹配这个屏幕上的号码。';

  @override
  String get peerVerifyOwnNumber => '此设备上的数字';

  @override
  String get peerVerifyEnterNumber => '该同事宣读的数字';

  @override
  String get peerVerifyNoPaste => '键入数字——故意禁用粘贴.';

  @override
  String get peerVerifyQrHelp => '亲自: 向同事显示这个QR 代码, 用手机扫描他们的。在没有相机的计算机上, 请输入同事的QR 代码。';

  @override
  String get peerVerifyQrInput => '同事QR代码文本';

  @override
  String get peerVerifyConfirm => '校验';

  @override
  String get peerVerifySuccess => '设备已验证';

  @override
  String get sharingAcceptOwnerHelp => '接受将所有者的密钥钉在这个设备上。与所有者一起验证, 以标记已核实的项目。';

  @override
  String get sharingOwnerUnverified => '所有者未经核实';

  @override
  String get sharingOwnerUnverifiedNote => '此项目的主机从未自动连接, 片段显示其来源。';

  @override
  String get sharingVerifyBannerTitle => '核查可接触秘密的同事';

  @override
  String get sharingVerifyBannerMessage => '这些秘密的收件人Kubernetes 上下文和数据库连接被添加后, 才能验证密钥, 其访问权限不变。';

  @override
  String get sharingVerifyBannerDismiss => '隐藏';

  @override
  String get sharingPolicyTitle => '共享:仅核实的收件人';

  @override
  String get sharingPolicySensitive => '机密, Kubernetes 上下文和数据库连接';

  @override
  String get sharingPolicyOther => '主机、组和片段';

  @override
  String get sharingPolicyHelp => '新的访问和新设备需要验证密钥。现有接收者保留其访问权限。';

  @override
  String get enrollmentTypeCode => '其他设备上显示的代码';

  @override
  String get enrollmentTypeCodeHelp => '从其它设备的屏幕中输入六组数字。代码永远不会被填入。';

  @override
  String get vaultKeyMismatchDiagnostics => '诊断';

  @override
  String get dbDesignIdentityAutoincrement => '担保';

  @override
  String dbDesignIssueAutoincrementKey(String name) {
    return '$name: AUTOINCENT 单作为排行表的INTEGER主键需要这一列.';
  }

  @override
  String get dbDesignIssueAutoincrementSingle => '只有一个栏目可以有自动。';

  @override
  String dbDesignIssueRebuildBlocked(String name) {
    return '此更改重现了表格,表格有设计者不结转的内容:$name. Make it in the SQL editor.';
  }

  @override
  String dbDesignIssueTableRebuild(String name) {
    return '$name 将重新创建: 将行复制到一个新的表格中, 并丢弃旧的表格, 全部在一个交易中。表格的触发器没有保存, 所以如果有更改, 则拒绝更改。';
  }

  @override
  String dbDesignIssueTypeStrict(String name) {
    return 'TRICT表只收INT,INTEGER,REAL,TEXT,BLOB和Anyone: $name.';
  }

  @override
  String get dbDesignIssuePrimaryKeyRequired => '选择主密钥列。';

  @override
  String dbDesignIssueCommentIgnored(String name) {
    return '$name : 此数据库不保存注释; 注释没有保存。';
  }

  @override
  String dbDesignIssueColumnRename(String name) {
    return '只有主键列可以重命名为: $name.';
  }

  @override
  String dbDesignIssueTypeChange(String name) {
    return '列类型不能更改:$name 。';
  }

  @override
  String dbDesignIssueCounterMixed(String name) {
    return '$name : 计数列不能在键中,一个有计数台的表格除了键外,只有计数台.';
  }

  @override
  String get dbDesignIssueIndexSingleColumn => '二级指数包括一栏。';

  @override
  String get dbDesignUnmodeledTriggers => '触发';

  @override
  String get dbDesignUnmodeledGenerated => '生成的列';

  @override
  String get dbDesignUnmodeledCollations => '列整理';

  @override
  String get dbDesignUnmodeledIndexes => '表达式、部分索引或顺序索引';

  @override
  String get dbDesignUnmodeledOther => '设计者无法读取的约束';

  @override
  String get dbDesignOptStrict => 'STTRIC(限制栏类型)';

  @override
  String get dbDesignOptUnlogged => 'UNLOGGED( 无写头日志 : 更快, 崩溃后空)';

  @override
  String get dbDesignIssueUnloggedPartitioned => '分隔的表格不能是UNLOGGED。';

  @override
  String get dbDesignOptWithoutRowid => '没有零散';

  @override
  String get dbDesignOptPartitionColumns => '分区密钥中的密钥列';

  @override
  String get dbDesignOptClusteringOrder => '集群顺序(例如在DESC)';

  @override
  String get dbDesignOptDefaultTtl => '默认 TTL, 秒';

  @override
  String get dbDesignOptGcGrace => 'gc_grace_秒数';

  @override
  String get aiHistoryTitle => '对话历史';

  @override
  String get aiHistoryEmpty => '还没有对话。要在保险库锁后保持对话, 请在设置中打开“ 在保险库中保持 AI 对话 ” 。';

  @override
  String get aiHistoryFailed => '无法装入对话框。';

  @override
  String get aiHistoryUntitled => '无标题的对话';

  @override
  String get aiHistoryKept => '藏在保险库里';

  @override
  String get aiHistoryUntilLock => '直到保险库锁';

  @override
  String get aiHistoryProviderGone => '其提供者已删除';

  @override
  String aiHistoryMessages(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count电文', one: '1条信息');
    return '$_temp0';
  }

  @override
  String syncVaultDeletionTitle(String date) {
    return '这个保险库将在$date上删除';
  }

  @override
  String get syncVaultDeletionBody => '您账户中另一个尚未被信任的设备请求删除它。如果不是您, 请保存库并更改您的账户密码。';

  @override
  String get syncVaultDeletionCancel => '保管保险库';

  @override
  String get syncVaultDeletionCancelled => '删除被取消。';

  @override
  String get dbMoveToVault => '与保险库同步';

  @override
  String get dbMoveToVaultHelp => '连接及其密码会移动到保险库中: 它们会同步到您的所有设备, 并用保险库备份。它不会只返回到此设备。';

  @override
  String get dbMoveToVaultConfirm => '搬进保险库';

  @override
  String get dbMovedToVault => '连接与保险库同步。';

  @override
  String get dbMoveOutdatedTitle => '先更新设备';

  @override
  String get dbMoveOutdatedBody => '在 0. 9.2 之前的 Apps 无法读取此连接。更新您的账户中的这些设备( 或签名退出), 然后再次尝试 :';

  @override
  String get dbMoveVersionUnknown => '版本未知';

  @override
  String get aboutServerSource => '您同步服务器的源代码 :';

  @override
  String get startupFailedTitle => 'ConsoleCrypt 无法启动安全核心';

  @override
  String get startupFailedBody => '您的数据没有被触动。重新安装应用程序或用安装的 Rust 工具链重建它, 然后再试一次。';

  @override
  String get startupFailedStorageBody => '无法安全打开现有存储。您的数据没有更改。请解决下面的存储问题, 再试一次。';

  @override
  String get snippetPackSqlite => 'SQLite';

  @override
  String get snippetPackSqliteDescription => 'Schema, 索引, 外国密钥, 完整性, 大小和查询计划SQLite 文件。对于 sqlite3 或数据库的工作空间。';

  @override
  String get snippetDbSqliteTables => '表和意见';

  @override
  String get snippetDbSqliteTablesNote => '每个表格,查看,索引和触发文件,除了内部的sqlite_。';

  @override
  String get snippetDbSqliteColumns => '表格的列数';

  @override
  String get snippetDbSqliteColumnsNote => '名称,声明类型,NOT NULL,默认,主密钥位置和隐藏(生成)旗帜.';

  @override
  String get snippetDbSqliteIndexes => '表格索引';

  @override
  String get snippetDbSqliteIndexesNote => '每个具有独有性的索引,来源(c = CREATE INDEX,u = UNIQUE,pk)以及是否为部分.';

  @override
  String get snippetDbSqliteIndexColumns => '索引的列数';

  @override
  String get snippetDbSqliteIndexColumnsNote => '索引按顺序排列,并附有排序方向和整理。';

  @override
  String get snippetDbSqliteForeignKeys => '表格的外国密钥';

  @override
  String get snippetDbSqliteForeignKeysNote => '参考表格和列,有最新数据 / 关于DEELETE 动作。';

  @override
  String get snippetDbSqliteFkCheck => '切换外来密钥的行';

  @override
  String get snippetDbSqliteFkCheckNote => '其外国密钥未点位置的行( 即使执行关闭也会检查) 。';

  @override
  String get snippetDbSqliteQuickCheck => '快速完整性检查';

  @override
  String get snippetDbSqliteQuickCheckNote => '整个文件的结构检查; 当没有问题时返回还好。';

  @override
  String get snippetDbSqliteSize => '文件大小和免费页面';

  @override
  String get snippetDbSqliteSizeNote => '正在使用的页面,页面大小,以字节表示的大小和页面 VACUUM 可以放行.';

  @override
  String get snippetDbSqliteDefinition => '对象的 CREATE 语句';

  @override
  String get snippetDbSqliteDefinitionNote => '创建表格、视图、索引或触发器的SQL,存储在文件中。';

  @override
  String get snippetDbSqliteCount => '表格中的行';

  @override
  String get snippetDbSqliteCountNote => '精确行数(扫描表格).';

  @override
  String get snippetDbSqlitePlan => '查询计划';

  @override
  String get snippetDbSqlitePlanNote => '如何运行查询SQLite:使用索引、扫描和临时B树。';

  @override
  String get snippetDbSqliteVersion => 'SQLite 版 (中文(简体) ).';

  @override
  String get snippetDbSqliteVersionNote => '打开文件的程序的库版本和源代码.';

  @override
  String get snippetDbSqliteCompileOptions => '编译选项';

  @override
  String get snippetDbSqliteCompileOptionsNote => '选项 SQLite 库是用 JSON, FTS5, 最大尺寸建造的。';

  @override
  String get snippetDbSqliteAnalyze => '更新规划师统计';

  @override
  String get snippetDbSqliteAnalyzeNote => '将索引统计收集到sqlite_stat1,所以规划者选择更好的索引.';

  @override
  String get snippetDbSqliteVacuum => '压缩文件';

  @override
  String get snippetDbSqliteVacuumNote => '重建文件以返回磁盘的自由页面;需要独家访问和关于文件大小的自由空间.';

  @override
  String get cliTitle => '命令行';

  @override
  String get cliHelp => '控制台crypt-cli命令管理配置文件,主机,密钥,隧道,SFTP,同步和终端备份,它的工作配置文件与应用程序相同;控制台crypt-cli--help列出命令.';

  @override
  String get cliInstall => '安装 CLI 命令';

  @override
  String get cliUninstall => '删除 CLI 命令';

  @override
  String cliInstalled(String location) {
    return '已安装:$location';
  }

  @override
  String get cliNotInstalled => '控制台crypt-cli 命令尚未安装。';

  @override
  String get cliNotBundled => '此架构没有控制台crypt-cli;它与下载页面的安装器一起出现.';

  @override
  String cliNotOnPath(String folder) {
    return '$folder不是在 PATH 上: 在您的 shell 设置中添加它, 从而找到命令。';
  }

  @override
  String get cliInstalledHint => '完成。打开一个新的终端窗口并运行控制台crypt-cli--help。';

  @override
  String get cliRemoved => 'CLI命令被移除.';

  @override
  String cliConflict(String path) {
    return '$path被另一个档案所收录; ConsoleCrypt离开它.';
  }

  @override
  String get cliFailed => '无法安装或删除 CLI 命令。';

  @override
  String get terminalPreviousTab => '上一个终端标签';

  @override
  String get terminalNextTab => '下一个终端标签';

  @override
  String get sharingTeamsHelp => '各小组、其成员和他们可用的资源。';

  @override
  String sharingTeamResources(int count) {
    String _temp0 = intl.Intl.pluralLogic(count, locale: localeName, other: '$count资源', one: '1个资源');
    return '$_temp0';
  }

  @override
  String get sharingTeamNoResources => '这个团队没有资源。';

  @override
  String get sharingTeamLocked => '解锁团队保险库查看其成员和资源.';

  @override
  String get keepassTitle => 'KeePass 文件';

  @override
  String get keepassHelp => '打开本地KDBX 3/4 文件或连接一个WebDAV 数据库。打开数据库可以保留到 Ctrl+K。在锁定前保存更改; 本地保存保留在原始文件旁边的加密备份。';

  @override
  String get keepassOpen => '打开KDBX';

  @override
  String get keepassCreate => '新设KDBX';

  @override
  String get keepassPassword => '数据库密码';

  @override
  String get keepassConfirm => '重复数据库密码';

  @override
  String get keepassKeyFile => '选择密钥文件( 可选)';

  @override
  String get keepassClearKey => '删除密钥文件';

  @override
  String get keepassNewEntry => '添加条目';

  @override
  String get keepassNewGroup => '添加组';

  @override
  String get keepassEntry => 'KeePass条目';

  @override
  String get keepassGroup => '组';

  @override
  String get keepassGroupName => '组名称';

  @override
  String get keepassEntryTitle => '标题';

  @override
  String get keepassUsername => '用户名';

  @override
  String get keepassEntryPassword => '密码';

  @override
  String get keepassUrl => '網址';

  @override
  String get keepassNotes => '笔记';

  @override
  String get keepassSearch => '搜索条目';

  @override
  String get keepassDirty => '未保存的更改';

  @override
  String get keepassSaved => '已加密文件已保存';

  @override
  String get keepassDiscard => '丢弃更改';

  @override
  String get keepassDiscardHelp => '关闭此文件并丢弃未保存的更改 ?';

  @override
  String get keepassDelete => '将条目移动到回收箱吗 ?';

  @override
  String get keepassDeleteHelp => '该条目将保留在数据库的回收箱中。更改会自动保存。';

  @override
  String get keepassKeyError => '密码或密钥文件不正确。请检查两者并再次尝试。';

  @override
  String get keepassFormatError => '无法打开数据库。数据库可能损坏或使用不支持的格式。原始文件没有更改。';

  @override
  String get keepassUnsupported => '此数据库的一些特性或设置尚未支持。原始文件没有更改。';

  @override
  String get keepassChanged => '文件在磁盘上更改了。保存已被取消以保护该版本。重新打开文件以载入最新的更改。';

  @override
  String get keepassIoError => '无法读取或保存文件。请检查访问权限和磁盘空格。';

  @override
  String get keepassClosed => 'KeePass 会话已锁定。用密码重新打开文件。';

  @override
  String get keepassLimits => '文件太大了或者它的关键衍生需要太多的资源.';

  @override
  String get keepassExists => '文件已经存在。选择新文件名。';

  @override
  String get keepassMismatch => '密码不匹配。';

  @override
  String get keepassEmpty => '此组中没有条目';

  @override
  String get keepassLock => '锁定数据库';

  @override
  String errorWindowsHelloDiagnostic(String diagnostic) {
    return 'Windows Hello 失败 $diagnostic. 如果指纹验证失败, 请在 Windows 安全对话框中尝试 PIN。保险库口令仍然可用。';
  }

  @override
  String get navPasswords => '密码';

  @override
  String get keepassDavAdd => '连接WebDAV';

  @override
  String get keepassDavUrl => 'KDBX 文件 URL HTTPS ).';

  @override
  String get keepassDavName => '连接名称';

  @override
  String get keepassDavPassword => 'WebDAV 密码/ app 代号';

  @override
  String get keepassDavHelp => 'WebDAV 连接设置和凭据会加密并随 ConsoleCrypt 保险库同步。KDBX 文件仍保存在 WebDAV 上。';

  @override
  String get keepassDavUrlError => '使用一个.kdbx文件的完整HTTPSURL,例如https://server/folder/Passwords.kdbx.服务器或文件夹URL是不够的.';

  @override
  String get keepassDavAuthError => 'WebDAV 身份验证失败。请检查连接凭据。';

  @override
  String get keepassDavEtagError => '此服务器既不提供强大的ETag, 也不提供可用的文件锁。请保存本地副本以保留您的更改。';

  @override
  String get keepassDavRedirectError => 'WebDAV 重定向请求。使用最终文件 URL。';

  @override
  String get keepassDavIoError => '无法访问 WebDAV 文件。请检查地址、网络和权限。';

  @override
  String get keepassUnlock => '解锁数据库';

  @override
  String get keepassUnlockOthers => '解锁其他数据库';

  @override
  String get updateRuStoreHelp => '此版本通过 RuStore 更新。请检查并在应用程序商店中安装更新。';

  @override
  String get updateOpenRuStore => '打开 RuStore';

  @override
  String get terminalOpenSftp => '打开SFTP';

  @override
  String get keepassDatabases => '数据库';

  @override
  String get keepassRecentDatabases => '最近打开的数据库';

  @override
  String get keepassDavDatabases => 'WebDAV数据库';

  @override
  String get keepassHello => '生物计量/装置PIN';

  @override
  String get keepassSecurityKey => 'FIDO2安全钥匙';

  @override
  String get keepassQuickHelp => '先用主密码打开数据库。然后启用本地密钥，由设备的生物识别或 PIN 保护，也可以使用支持 PRF 的 FIDO2 安全密钥。此设置仅适用于当前设备。';

  @override
  String get keepassQuickEnabled => '已启用快速解锁';

  @override
  String get keepassQuickDisabled => '停用快速解锁';

  @override
  String get keepassProtection => '快速解锁';

  @override
  String get keepassPin => '安全钥匙PIN';

  @override
  String get keepassCopyPassword => '复制密码';

  @override
  String get keepassCopyUsername => '复制用户名';

  @override
  String get keepassCopyField => '复制字段';

  @override
  String get keepassField => '外地';

  @override
  String get keepassValue => '数值';

  @override
  String get keepassFieldAdd => '添加字段';

  @override
  String get keepassExport => '保存本地副本';

  @override
  String get keepassAllGroups => '所有条目';

  @override
  String get keepassPaletteHelp => '查找条目,然后选择要复制的字段。';

  @override
  String get keepassPaletteBack => '返回到所有命令';

  @override
  String get keepassOpenManager => '打开密码管理器';

  @override
  String get keepassConnectDelete => '删除连接';

  @override
  String get keepassConnectDeleteHelp => '从同步设置中移除此连接 ? KDBX 文件将保留在 WebDAV 上。';

  @override
  String get keepassGroups => '分组';

  @override
  String get keepassFieldNameError => '字段名称必须是独一无二的,不能使用标准字段名称.';

  @override
  String get keepassWelcome => '密码管理器';

  @override
  String get keepassWelcomeHelp => '打开或新建 KDBX 数据库，也可以连接 WebDAV 上的文件。';

  @override
  String get keepassAddDatabase => '添加数据库';

  @override
  String get keepassNoSelection => '选择查看其牌的条目。';

  @override
  String get keepassDropHint => '将条目拖放到文件夹中移动。';

  @override
  String get keepassUngrouped => '未分组';

  @override
  String get keepassAutosaving => '保存更改...';

  @override
  String get keepassAutosaved => '所有更改自动保存';

  @override
  String get keepassRetrySave => '重试保存';

  @override
  String get keepassChooseField => '选择要复制的内容';

  @override
  String get keepassBackEntries => '返回条目';

  @override
  String get keepassSelectCard => '打开进入卡';

  @override
  String get keepassMore => '数据库操作';

  @override
  String get keepassDavTimeout => 'WebDAV 请求超时。请检查文件 URL 和连接。';

  @override
  String get keepassDavLocked => '此文件被另一个客户端锁定。稍后再尝试保存。';

  @override
  String get keepassResizeGroups => '调整文件夹的大小';

  @override
  String get keepassResizeCard => '调整入场卡大小';

  @override
  String get keepassGenerator => '密码生成器';

  @override
  String get keepassGeneratorLength => '长度(8-128)';

  @override
  String get keepassGeneratorUpper => '大写( A- Z)';

  @override
  String get keepassGeneratorLower => '小写 (a-z)';

  @override
  String get keepassGeneratorDigits => '数字( 0 - 9)';

  @override
  String get keepassGeneratorSymbols => '符号 (! @ #...)';

  @override
  String get keepassGeneratorSimilar => '不包括类似字符( 0 O 1 I )';

  @override
  String get keepassGeneratorRegenerate => '再次生成';

  @override
  String get keepassGeneratorUse => '使用密码';

  @override
  String get keepassGeneratorLengthError => '选择从8到128的长度.';

  @override
  String get keepassGeneratorGroupsError => '选择至少一个字符类。';

  @override
  String keepassGeneratorEntropy(String bits) {
    return '生成 en: $bits位';
  }

  @override
  String get keepassSelectSource => '选择数据库';

  @override
  String get keepassRemoveSource => '从列表中删除';

  @override
  String keepassRemoveLocalHelp(String name) {
    return '关闭“$name”并从此设备的列表中移除？KDBX 文件仍保存在磁盘上。';
  }

  @override
  String keepassRemoveRemoteHelp(String name) {
    return '关闭并移除“$name”连接？文件仍保存在 WebDAV 上，连接设置将从已同步的设备中移除。';
  }

  @override
  String get keepassGroupActions => '集体行动';

  @override
  String get keepassRenameGroup => '重命名组';

  @override
  String keepassDeleteGroup(String name) {
    return '将“$name”组移至回收箱?';
  }

  @override
  String get keepassDeleteGroupHelp => '组、子组及其所有条目将移动到KDBX 回收站。更改会自动保存。';

  @override
  String get keepassGroupUnavailable => '无法更改此组。 root 组和系统回收站被保护。';

  @override
  String get keepassGroupContainsBin => '此文件夹包含KDBX 回收桶。';

  @override
  String get keepassEmptyTrash => '空回收桶';

  @override
  String get keepassEmptyTrashHelp => '永久删除此KDBX 中的所有组、嵌套组和条目 ? 无法在数据库中撤销。活动组和条目将被保存。更改会自动保存。';

  @override
  String get keepassDeletePermanently => '永久删除';

  @override
  String keepassPurgeGroup(String name) {
    return '永久删除“$name”组?';
  }

  @override
  String get keepassPurgeGroupHelp => '此组、所有嵌套组及其所有条目将从数据库中永久删除KDBX。这无法在数据库中撤销。更改会自动保存。';

  @override
  String get keepassPurgeEntry => '永久删除此条目?';

  @override
  String get keepassPurgeEntryHelp => '此条目将从KDBX 数据库中永久删除。无法在数据库中撤销。更改会自动保存。';

  @override
  String get terminalMinimalMode => '最小化';

  @override
  String get terminalMinimalExit => '退出最小化';

  @override
  String get terminalMinimalOptions => '终端布局和工具';

  @override
  String get terminalMinimalTabsTop => '顶端的标签';

  @override
  String get terminalMinimalTabsLeft => '左边的标签';

  @override
  String get keepassEntryViews => '条目';

  @override
  String get keepassUngroupedHelp => '存储在数据库根上的条目, 没有文件夹。';

  @override
  String get keepassRecycleBin => '回收桶';

  @override
  String get sftpDefaultEditorTitle => 'SFTP 默认编辑器';

  @override
  String get navigationAllSections => '所有功能';

  @override
  String get navigationCustomize => '自定义侧边栏';

  @override
  String get navigationCustomizeHint => '为当前配置文件而绑定的此设备的段落。隐藏的段落在所有段落中仍然可用。';

  @override
  String get navigationRestore => '恢复默认侧边栏';

  @override
  String get navigationFutureVersion => '这些侧边栏设置是由更新版本ConsoleCrypt 创建的。更新应用程序以编辑它们。';

  @override
  String get automationTitle => '自动化';

  @override
  String get automationUnavailable => '此建筑中没有自动化功能。';

  @override
  String get automationNewProject => '新建项目';

  @override
  String get automationOpenFolder => '打开文件夹';

  @override
  String get automationProjects => '项目';

  @override
  String get automationNewFile => '新文件( 项目路径)';

  @override
  String get automationChooseFile => '选择要编辑的项目和文件。';

  @override
  String get automationProjectHint => '在不安装 Ansible 的情况下创建和编辑Playbook。文件会留在此设备上。';

  @override
  String get automationNoProjects => '还没有项目。';

  @override
  String get automationNewerSchema => '此目录由更新的版本创建。无法编辑。';

  @override
  String get automationUnsaved => '要保存此文件的更改吗 ?';

  @override
  String get automationDiscard => '丢弃';

  @override
  String get automationFileChanged => '文件在外部ConsoleCrypt 更改。您的编辑仍然保留在编辑器中。在再次保存前比较修改。';

  @override
  String get automationRunners => 'Runner 环境';

  @override
  String get automationRunnerDetails => '组件和隔离';

  @override
  String get automationRunnerComponents => '构成部分';

  @override
  String get automationRunnerIsolation => '环境检查';

  @override
  String get automationRunnerReady => '环境准备就绪';

  @override
  String get automationRunnerNeedsSetup => '所需准备';

  @override
  String get automationRemoteNonRootHint => '使用正则Linux账户选择一个SSH连接,而不是root。在终端中准备带有sudo的系统软件包; Runner 环境本身安装时没有root。';

  @override
  String get automationSetupExecutableHome => '一个允许装入可执行库( 不是执行) 的家庭文件系统';

  @override
  String get automationRunnerHint => '连接已准备好的本地环境或 Linux 服务器。检查过程不会安装组件或运行 Playbook。';

  @override
  String get automationMobileRunnerRequired => '在电话中,运行使用一个Linux Runner over SSH. 从您保存的SSH主机中添加一个 Runner。';

  @override
  String get automationNoRunners => '没有保存的 Runner 环境。';

  @override
  String get automationRemoteRunner => 'Linux 超SSH(人工运行)';

  @override
  String get automationLocalRunner => '本地 Runner';

  @override
  String get automationDiagnose => '检查环境';

  @override
  String get automationAddRunner => '添加环境';

  @override
  String get automationDependencyLock => '依赖性匹配锁定文件';

  @override
  String get automationNonRoot => '无根运行';

  @override
  String get automationFilesystemIsolation => '文件系统隔离';

  @override
  String get automationNetworkIsolation => '网络隔离';

  @override
  String get automationProcessIsolation => '进程隔离';

  @override
  String get automationProbeOnly => '这些是环境诊断,执行需要单独的项目和目标验证以及工作批准。';

  @override
  String get automationPreparedEnvironment => '在安装运行器的机器上输入绝对路径。';

  @override
  String get automationDistribution => '选择一个WSL 分布';

  @override
  String get automationLinuxUser => 'Linux用户(不是根)';

  @override
  String get automationChooseRunnerHost => '选择运行者SSH 主机';

  @override
  String get automationPythonPath => '环境 Python 路径';

  @override
  String get automationModulePath => '运行器模块目录';

  @override
  String get automationStatePath => '私有 Runner 状态目录';

  @override
  String get automationRequiredFields => '完成字段并选择连接环境。';

  @override
  String get automationDisconnectRunner => '断开运行器频道';

  @override
  String get automationForgetRunner => '删除连接配置';

  @override
  String get automationCheckReady => '准备';

  @override
  String get automationCheckUnavailable => '无法获取';

  @override
  String get automationRun => '检查并运行';

  @override
  String get automationJobs => '任务';

  @override
  String get automationRunSetup => '任务设置';

  @override
  String get automationChooseRunner => '选择运行者';

  @override
  String get automationPlaybook => 'Playbook';

  @override
  String get automationTargets => '任务目标';

  @override
  String get automationVariables => '变量 JSON 对象';

  @override
  String get automationTags => '任务标记, 逗号分隔';

  @override
  String get automationSkipTags => '跳过标签';

  @override
  String get automationTimeout => '超时, 秒';

  @override
  String get automationForks => '并行任务(1-5)';

  @override
  String get automationCancelOnLock => '保险库锁定时取消';

  @override
  String get automationCheckMode => '检查模式';

  @override
  String get automationApplyMode => '应用模式';

  @override
  String get automationCheckWarning => '检查也可以改变服务器:单个任务可以忽略检查模式.';

  @override
  String get automationDiff => '在结果中包含 diffs( 可能包含敏感数据)';

  @override
  String get automationValidate => '验证项目和目标';

  @override
  String get automationReview => '审查工作';

  @override
  String get automationConfirm => '确认执行';

  @override
  String get automationReviewHint => '此精确的快照将会执行。文件、目标或环境的更改需要新的审查。';

  @override
  String get automationValidationFailed => '验证失败。更新项目并再次验证。';

  @override
  String get automationRevision => '项目简介';

  @override
  String get automationEnvironment => '环境';

  @override
  String get automationShowVariables => '显示变量';

  @override
  String get automationHideVariables => '隐藏变量';

  @override
  String get automationReviewExpires => '审查期满';

  @override
  String get automationRunFields => '选择一个 Runner、一个 Playbook 和至少一台 SSH 主机。变量必须是 JSON 对象。';

  @override
  String get automationNoJobs => '还没有工作';

  @override
  String get automationCancelJob => '取消任务';

  @override
  String get automationUnknownWarning => '结果未知: 命令可能仍在运行, 更改也可能是局部的。重叠的任务会一直被阻塞直到和解。';

  @override
  String get automationAcknowledgeUnknown => '确认可能的变化';

  @override
  String get automationAcknowledgeConfirm => '我检查了目标,了解了局部变化的风险,允许新的工作。';

  @override
  String get automationHistoryHint => '队列存储在您的加密配置文件中。未完成的任务在重启后不会重播。';

  @override
  String get automationStateQueued => '队列';

  @override
  String get automationStateDispatching => '调度';

  @override
  String get automationStateRunning => '运行';

  @override
  String get automationStateSucceeded => '已经成功';

  @override
  String get automationStateFailed => '失败';

  @override
  String get automationStateCancelled => '已取消';

  @override
  String get automationStateInterrupted => '中断';

  @override
  String get automationStateUnknown => '未知结果';

  @override
  String get automationStateAcknowledged => '确认';

  @override
  String get automationEvents => '任务和活动';

  @override
  String get automationShowEvents => '公开活动';

  @override
  String get automationMoreEvents => '以后的活动';

  @override
  String get automationPreviousEvents => '以往活动';

  @override
  String get automationFollowEvents => '跟踪执行';

  @override
  String get automationOutputLimited => '输出被配置的限制截断。';

  @override
  String get automationNoEvents => '未发生任何事件';

  @override
  String get automationRoute => 'SSH路线';

  @override
  String get automationOrigin => '连接到目标';

  @override
  String get automationLocalOrigin => '这台电脑';

  @override
  String get automationRemoteOrigin => '远程 Runner';

  @override
  String get automationReviewDetails => '快照技术细节';

  @override
  String get automationKeepRunningOnLock => '锁定时, 已经运行的工作将持续到访问结束。';

  @override
  String get automationAiSelectedContext => '文件上下文: 发送前删除不想要的部分';

  @override
  String get automationAiTitle => '用 AI 创建或编辑Playbook';

  @override
  String get automationAiExactRequest => '精确模式请求';

  @override
  String get automationAiChanges => '变动';

  @override
  String get automationAiBefore => '在此之前';

  @override
  String get automationAiAfter => '之后';

  @override
  String get automationAiAcceptChanges => '接受更改';

  @override
  String get automationAiRejectChanges => '拒绝';

  @override
  String get automationAiDraftStale => '文件或项目在AI准备答案时有所改变, 草稿没有应用。打开要的文件并再次比较。';

  @override
  String get automationAiTask => '描述任务和结果检查';

  @override
  String get automationAiIncludeFile => '包含打开的文件文本';

  @override
  String get automationAiContextHint => '只有您的任务和所选文件被发送。请检查文本: 过滤无法检测每个秘密。';

  @override
  String get automationAiPreview => '预览上下文';

  @override
  String get automationAiPreviewHint => '这是完整的过滤请求。返回任务以更改它; 只有下面的按钮发送它。';

  @override
  String get automationAiGenerate => '发送到选中的 AI';

  @override
  String get automationAiDraftHint => '这是一个提议。审查更改、所需的变量和依赖性。保存、检查和执行是单独的步骤。';

  @override
  String get automationAiUseDraft => '插入到编辑器中';

  @override
  String get automationAiOnDevice => '在这个设备上';

  @override
  String get automationAiNetworkProvider => '网络提供者';

  @override
  String get automationGitLocalChanges => '相对于进口承诺,当地有变化。';

  @override
  String get automationGitReady => '导入完成';

  @override
  String get automationGitImport => '从Git进口';

  @override
  String get automationGitName => '项目名称';

  @override
  String get automationGitHint => '该计算机上的Git存储器将一个HTTP或HTTPS存储器导入您的本地ConsoleCrypt配置文件。该存储器必须无证书可读。';

  @override
  String get automationGitUrl => '仓库 URL HTTP 或 HTTPS )';

  @override
  String get automationGitReference => '分支或标签';

  @override
  String get automationGitCustomCa => '使用自定义证书权限';

  @override
  String get automationGitCa => 'PEM 的公用 CA 证书( 无私钥)';

  @override
  String get automationGitFields => '输入项目名称、仓库 URL 和分支或标记。';

  @override
  String get automationGitReceiving => '接收和校验文件...';

  @override
  String get automationGitFailed => '导入未完成。请检查运行器上的 URL、分支或标记、证书和 Git。无法执行部分项目。';

  @override
  String get automationRetryJob => '准备新跑步';

  @override
  String get automationRetryOrigin => '上一个工作';

  @override
  String get automationRetryUnavailable => '项目或Playbook不再可供新运行.';

  @override
  String get automationRetryWarning => '这是一个新任务。请检查先前的结果 : 可能已经应用过更改。再次选择运行器、目标和变量; 需要验证和确认。';

  @override
  String get automationGroups => 'Ansible个团体)';

  @override
  String get automationGroupName => '组名称';

  @override
  String get automationGroupsHint => '使用Playbook中的组名来瞄准一组主机。运行时选择实际主机。';

  @override
  String get automationGroupsInvalid => '使用一个最多96个字符的独特组名: ASCII 字母、数字、连字符或下划线; 以字母或下划线开头。名称全部保留, 未分组和_meta。最多64组。';

  @override
  String get automationGroupMissing => '主机不再可用;不检查以取消其组员资格.';

  @override
  String get automationInventorySavedSearch => '搜索已保存的 SSH 主机';

  @override
  String get automationInventorySources => '清单来源';

  @override
  String get automationEditRunnerHint => '保存断开闲置的 Runner。活动任务必须先完成; 重新连接并检查编辑后的环境。';

  @override
  String get automationAiTruncated => '模型在完成Playbook前停止了。尝试一个较小的更改或更少的文件上下文; 没有保存文件。';

  @override
  String get automationAiInvalidResponse => '该模型没有以要求的格式返回完整的播放簿建议。尝试一个较小的任务或其他模型。没有保存或执行文件。';

  @override
  String get automationAiModelMissing => '未找到所选模式。请检查提供者模式设置。';

  @override
  String get automationAiProviderRejected => 'AI 提供者拒绝生成。请检查它的模型和上下文设置; 没有保存文件。';

  @override
  String automationAiHttpError(String status) {
    return 'AI 提供者返回 HTTP$status. 请检查它的模型, 上下文大小和请求设置, 没有保存文件。';
  }

  @override
  String get automationInventorySourcesHint =>
      '选择 ConsoleCrypt 中的主机与文件夹、项目清单文件，或同时使用两者。预览不会在目标服务器上运行任务。每次只会对你明确选择的目标执行任务。';

  @override
  String get automationRunnerSshRouteHint => '选择保存的SSH 主机。它的跳转主机和主机密钥检查也适用于运行器。目标连接从选中的运行器开始; 此计算机本地主机上的隧道可能无法到达。';

  @override
  String get automationInventoryHint => '选择项目文件和读取它们的运行者。预览不会在目标服务器上运行任何内容。将每个条目绑定到保存的SSH 主机; 任务只接收明确选中的目标。';

  @override
  String get automationInventoryFiles => 'INI/YAML文件或目录(1至8个来源,最多128个文件)';

  @override
  String get automationInventoryDraftHint => '选择不同的源重置草稿绑定。保存的配置仅在保存后更改。';

  @override
  String get automationInventoryPreview => '预览';

  @override
  String get automationInventoryChanged => '项目或配置改变。关闭此对话框并重新打开目录源。';

  @override
  String get automationInventoryFailed => '无法编制目录。请检查源和运行器环境。';

  @override
  String get automationInventoryMissingFile => '文件已不在本项目中; 从选中中删除。';

  @override
  String get automationInventoryMode => '清单目标';

  @override
  String get automationInventoryExternalOnly => '只有明确绑定的源项';

  @override
  String get automationInventoryMixed => '混合';

  @override
  String get automationInventoryBound => '明确约束力';

  @override
  String get automationInventoryBind => '捆绑到SSH 主机';

  @override
  String get automationInventoryUnbound => '不绑定到主机';

  @override
  String get automationInventoryMissingHost => '无法保存主机 —— 选择另一个或删除绑定';

  @override
  String get automationInventoryRebind => '来源已更改—— 重新绑定此条目';

  @override
  String get automationInventoryUseSaved => '使用所保存的主机地址、端口和用户, 当它们与源不同时';

  @override
  String get automationInventorySearch => '搜索别名、地址或用户';

  @override
  String get automationInventoryBack => '返回源选择';

  @override
  String get automationInventoryClear => '断开外部清单';

  @override
  String get automationInventoryClearHint => '只删除此项目的绑定和源选择 ? 项目文件和保存的主机仍然保留。未来运行将使用选中的ConsoleCrypt 主机。';

  @override
  String get automationInventoryPrevious => '上一个';

  @override
  String get automationInventoryNext => '下一步';

  @override
  String get automationInventoryConflicts => '捆绑需要注意';

  @override
  String get automationInventoryConflictsHint => '显示多达 50 期。修改后再按“ 保存” 键检查剩下的绑定值。更改尚未保存。';

  @override
  String get automationInventorySourceMissing => '条目已不在选定来源中';

  @override
  String get automationInventoryRouteInvalid => '已保存的主机没有有效的SSH 路由或用户';

  @override
  String get automationInventoryEndpointMismatch => '源参数与所保存的主机不同。审查时明确选择使用所保存的路由。';

  @override
  String get automationInventorySavedRoute => '已保存路线';

  @override
  String get automationInventoryShowEntry => '显示条目';

  @override
  String get automationInventoryResolved => '已抓取的清单文件';

  @override
  String get automationGitDiff => '项目变动';

  @override
  String get automationGitDiffHint => '将审查过的快照与原始树Git 相比较。没有任何东西被重置或推向 Git。';

  @override
  String get automationGitDiffTruncated => '显示前32,768个字符,打开项目编辑器获取完整文件.';

  @override
  String get automationGitDiffOriginal => '原树Git';

  @override
  String get automationGitDiffCurrent => '当前快照';

  @override
  String get automationGitDiffBinary => '二进制, 目录或文件大于 256 KiB 已更改。内容不预览。';

  @override
  String get automationGitDiffReviewed => '审查的变动';

  @override
  String get automationGitDiffAdded => '已添加';

  @override
  String get automationGitDiffDeleted => '删除';

  @override
  String get automationGitDiffModified => '修改(内容、类型或权限)';

  @override
  String get automationOpenEditor => '在外部编辑器中打开';

  @override
  String get automationReloadFile => '从磁盘重新装入文件';

  @override
  String get automationEditorSaveHint => '在打开外部编辑器之前保存您的更改。保存后, 请使用磁盘重装文件。运行一个任务需要单独的审查和确认。';

  @override
  String get automationRunnerUpdateForInputs => '更新选中的运行器并重新连接: 此版本不支持此项目所需的检查。';

  @override
  String get automationResultHosts => '核定东道方的结果';

  @override
  String get automationResultOk => '罢';

  @override
  String get automationResultChanged => '已更改';

  @override
  String get automationResultFailed => '失败';

  @override
  String get automationResultUnreachable => '无法达到';

  @override
  String get automationResultSkipped => '跳过';

  @override
  String get automationResultExitCode => '退出代码';

  @override
  String get automationResultMissing => '无数据';

  @override
  String get automationResultMissingHint => '只显示已核准的目标。缺少报告并不意味着成功零。';

  @override
  String get automationResultNoHosts => '这一遗留记录没有核定别名清单,技术细节见下文。';

  @override
  String get automationResultPartial => '一些修改可能已经完成;取消或断开不会将它们卷回。';

  @override
  String get automationResultEnvironment => '执行环境版本';

  @override
  String get automationResultEnvironmentHint => '工作被接受时录制.';

  @override
  String get automationResultEnvironmentMissing => '没有与本清单绑定的环境收据。当前版本不取代历史证据。';

  @override
  String get automationResultDetails => '技术细节';

  @override
  String get automationAiRequirementsTitle => '审查项目要求';

  @override
  String get automationAiRequirementsHint => '需要先保存, 然后保存Playbook。重生成保留已有的制约。依赖必须分别安装, 并被绑在锁文件中; 现在没有下载或执行任何内容。';

  @override
  String get automationAiRequirementsPartial => '要求已保存; Playbook没有。您的草稿仍然保存在编辑器中。解决冲突并再次保存; 其他编辑从不退缩。';

  @override
  String get automationTransitionTitle => '工作仍在进行中';

  @override
  String get automationTransitionJobs => '任务';

  @override
  String get automationTransitionHint => '新建任务暂时暂停。等待完成, 或取消任务并完成此动作。已经做出的更改不会自动回滚。';

  @override
  String get automationTransitionWaiting => '等待工人完成和清理。您可以在应用程序中停留; 已请求的取消不会被反转。';

  @override
  String get automationTransitionStay => '继续应用';

  @override
  String get automationTransitionWait => '等待完成时';

  @override
  String get automationTransitionCancelJobs => '取消任务并完成';

  @override
  String get automationTransitionUnknown => '成果不明的工作';

  @override
  String get automationTransitionHistoryUnavailable => '没有工作登记册,或者使用更新的格式。没有未完成的工作无法确认。';

  @override
  String get automationTransitionAcceptUnknown => '我理解: 命令可能仍在运行, 修改也可能是局部的。关闭不会标记这些任务成功或取消; 它们的状况需要单独审查。';

  @override
  String get automationSetupHint => '选择一个位置并找到一个环境。连接一个已有的运行器或安装一个单独的运行器。';

  @override
  String get automationDetectRunner => '查找环境';

  @override
  String get automationInstallRunner => '安装运行器';

  @override
  String get automationInstallRunnerPlan =>
      'ConsoleCrypt 在所选用户的目录和PyPI的下载封装Ansible包中创建了 Runner. Setup 验证 SHA-256 散列和沙盒,它需要互联网访问,可以长达14分钟.';

  @override
  String get automationSetupMissing => '所需先决条件';

  @override
  String get automationSetupPython => 'Python 3.12 或 3.14 搭载 Linux x64,或 Python 3.12 或 3.13 搭载 Linux ARM64';

  @override
  String get automationSetupNotFound => '找不到ConsoleCrypt 环境。安装新环境或手动配置已有环境。';

  @override
  String get automationSetupManual => '高级: 手动配置';

  @override
  String get automationSetupBusy => '正在检查或安装环境,结果将在此显示。';

  @override
  String get automationMacRunnerNative => '以Python 3.15或3.12. ConsoleCrypt为原产地在苹果硅上运行Ansible,准备单独的环境.';

  @override
  String get automationMacRunnerPython =>
      'ConsoleCrypt 无法为 Apple Silicon 找到 Python 3. 15 或 3. 12。安装 Homebrew 或 Python.org 的版本并再次搜索。';

  @override
  String get automationMacRunnerPythonUnsupported =>
      'Python被发现了,当地的Runner需要CPython 3.15或3.12从Homebrew或Python.org购买苹果硅.';

  @override
  String get automationMacRunnerArchitecture => '苹果硅( arm64)';

  @override
  String get automationMacRunnerIsolationFailed => 'Runner 设置无法验证macOS 沙盒或安装的软件包。';

  @override
  String get automationMacProcessIsolation => '工人过程保护';

  @override
  String get automationSelectAllTargets => '全选';

  @override
  String automationTargetLimit(int limit) {
    return '每项任务最多可选择 $limit 台主机。';
  }

  @override
  String get automationSetupFailed => '运行器设置失败。请检查 PyPI 访问和系统先决条件, 然后再次扫描。现有环境没有被替换。';

  @override
  String get automationSetupSystemHelp => '使用下面的按钮在选定的环境终端中运行此命令, 并输入您的 sudo 密码。在安装后再次扫描。您也可以复制命令。';

  @override
  String get automationSetupRunTerminal => '在环境中运行';

  @override
  String get automationInventorySavedHosts => 'ConsoleCrypt 主机';

  @override
  String get automationInventoryFilesOnly => '清单文件';

  @override
  String get automationInventorySavedHint => '选择主机或从文件夹中添加它们。当前的主机列表已被保存; 未来文件夹成员不会自动添加。执行前将检查有效的路径。';

  @override
  String get automationInventoryFolder => '从文件夹中添加主机';

  @override
  String get automationNewPlaybook => '新建Playbook';

  @override
  String get automationCreateRole => '创建角色';

  @override
  String get automationOtherFile => '其他文件';

  @override
  String get automationPlaybookPathHint => '使用一个以 yml 或 yaml 结尾的与项目相关的Playbook路径。';

  @override
  String get automationRoleNameHint => '角色名称:小写字母,数字和下划线;以字母开头,最多64个字符.';

  @override
  String get automationValidationTimedOut => '检查超过了时限。';

  @override
  String get automationValidationNoOutput => '检查未返回诊断输出。';

  @override
  String get automationPrerequisiteRescan => '环境或远程账户改变。在打开先决条件终端前再次扫描运行器。';

  @override
  String get automationPrerequisiteRouteUnavailable => '此路由没有先决条件的终端。为主机打开一个普通终端, 安装所需的组件, 然后再次扫描运行器。';

  @override
  String get automationPrerequisiteCommandRejected => '无法验证先决条件命令。没有执行命令。请再次扫描运行器以获取当前要求。';

  @override
  String get automationUpdateRunner => '更新运行器';

  @override
  String get automationRunnerNeedsUpdate => '此 Runner 需要更新当前目标检查。';

  @override
  String get automationRunnerUpdatePlanHint => '更新将在一个新的单独的目录中创建兼容的 Runner。已有的环境被保存。在安装后将新路径保存到 Runner 配置文件。';

  @override
  String get automationExecutionConsole => '执行';

  @override
  String get automationExecutionReadOnly => '只读任务日志';

  @override
  String get automationExecutionExpand => '扩展执行控制台';

  @override
  String get automationExecutionCollapse => '折叠执行控制台';

  @override
  String get automationExecutionResize => '调整执行控制台大小';

  @override
  String get automationExecutionTechnical => '技术活动详情';

  @override
  String get automationReviewPlaybookTitle => 'Playbook和模式';

  @override
  String get automationReviewRunnerTitle => '运行者和来源';

  @override
  String get automationReviewLimitsTitle => '运行策略';

  @override
  String get automationReviewApplyHint => '应用执行任务,并可以更改选定的服务器.';

  @override
  String automationReviewTargetCount(String count) {
    return '选中 : $count';
  }

  @override
  String get automationEventDiagnostics => '事件/单元诊断';

  @override
  String get automationEventCensored => '任务隐藏此输出( no_log) 。';

  @override
  String get automationReviewParallelism => '平行主义';

  @override
  String get automationConfigureRunner => '配置/ 更新运行器';

  @override
  String get automationRunnerRevalidate => '再次验证可重新连接到选中的 Runner 并准备新的审查。';

  @override
  String get automationExecutionOutput => '输出';

  @override
  String get automationExecutionTechnicalInformation => '技术信息';

  @override
  String get automationExecutionFollowLog => '跟踪任务日志';

  @override
  String get automationOpenOutput => '打开输出';

  @override
  String get automationJobDetails => '任务细节';

  @override
  String get automationConfigTitle => 'Ansible 设置';

  @override
  String get automationConfigPolicy => 'hosts and group,并行主义和信任SSH密钥由ConsoleCrypt管理. host_key_checking=False不禁用本地主机验证.';

  @override
  String get automationConfigPrecedence => '这些是项目默认值。 Playbook 关键字和变量可以覆盖它们。';

  @override
  String get automationConfigComplex => '模糊或复杂INI: 使用高级编辑器。文本没有更改。';

  @override
  String get automationConfigGathering => '事实收集';

  @override
  String get automationConfigGatherHelp => '每部剧目的隐含集合;明确要求剧目要求事实;智能在跑步中重复使用事实.';

  @override
  String get automationConfigImplicit => '隐性';

  @override
  String get automationConfigExplicit => '明确';

  @override
  String get automationConfigSmart => '聪明';

  @override
  String get automationConfigFatal => '在失败时停止所有主机';

  @override
  String get automationConfigFatalHelp => '默认任何_ errors_ fatal 策略; 播放簿错误处理仍然适用。';

  @override
  String get automationConfigTimeout => '任务动作超时, 秒';

  @override
  String get automationConfigTimeoutHelp => '0 禁用Ansible 动作超时。它排除了诱导和全循环; 工作截止时间ConsoleCrypt 仍然适用。';

  @override
  String get automationConfigInvalidTimeout => '从 0 输入整个数字。';

  @override
  String get automationConfigInherited => '此文件中未配置';

  @override
  String get automationConfigCustom => '自定义值: 选择替换或高级编辑器';

  @override
  String get automationConfigPreserve => '只有您更改的设置才会更新。注释和其他设置会被保存。';

  @override
  String get automationConfigYes => '对';

  @override
  String get automationConfigNo => '没有';

  @override
  String get automationConfigReset => '重置窗体变化';

  @override
  String get automationConfigAdvanced => '高级编辑器';

  @override
  String get automationConfigCancel => '取消';

  @override
  String get automationConfigApply => '应用更改';

  @override
  String get automationPasswordsHidden => '隐藏密码, 打开它们进行编辑。';

  @override
  String get automationPasswordsVisible => '密码可见.';

  @override
  String automationEditorExpandLines(int count) {
    return '扩展任务或块( $count 行)';
  }

  @override
  String automationEditorCollapseLines(int count) {
    return '折叠任务或块( $count 行)';
  }

  @override
  String get automationEditorLimit => '编辑器支持最多 1 MiB 的文件。此编辑没有应用。';

  @override
  String get automationAiRequirementsInvalid =>
      '无法合并项目要求。请检查. consolecrypt- requestments.json 中的格式和依赖性标点, 并使用这个精确的小写文件名。您的草稿仍在编辑器中。';

  @override
  String get automationCleanupTitle => '旧的管理运行器环境';

  @override
  String get automationCleanupScan => '扫描管理的环境';

  @override
  String get automationCleanupScope => '扫描检查管理环境位置ConsoleCrypt。只有已核实的管理目录可以批准删除。扫描和审查不会删除文件。';

  @override
  String get automationCleanupEmpty => '在此位置找不到管理的环境。';

  @override
  String get automationCleanupReview => '审查删除';

  @override
  String get automationCleanupConfirmTitle => '删除已审查的 Runner 环境 ?';

  @override
  String get automationCleanupConfirm => '删除此环境';

  @override
  String get automationCleanupCurrentProfile => '只检查了当前配置文件。其他配置文件没有检查, 可能仍然引用这些目录。整个配置文件的所有权未知。';

  @override
  String get automationCleanupStateNotice => '删除包括帮助器,Python环境以及Runner状态或以下每个精确目录内的工作数据.';

  @override
  String get automationCleanupExpiry => '此一次性使用批准有效期不超过60秒。在删除前再次检查目录和当前引用。';

  @override
  String get automationCleanupNodes => '文件系统条目';

  @override
  String get automationCleanupBytes => '已审查树上的字节';

  @override
  String get automationCleanupUnknownCount => '不详';

  @override
  String get automationCleanupQuarantine => '检疫目录';

  @override
  String get automationCleanupIdentity => '验证环境特性';

  @override
  String get automationCleanupTechnical => '收款详情';

  @override
  String get automationCleanupSuccess => '已审查环境已删除。计数描述已删除的文件系统条目和字节, 而不是自由磁盘空间。';

  @override
  String get automationCleanupInterrupted => '中断删除可以留下一个被隔离或部分删除的目录。再次扫描并审查新计划;删除永远不会自动重复。';

  @override
  String get automationCleanupUnsupported => '清理仅在此读取: app无法验证此环境的受保护管理目录。扫描支持验证的非根Linux, WSL 或 SSH 环境。';

  @override
  String get automationCleanupSelected => '目前选择的环境——在清理前选择另一个运行者.';

  @override
  String get automationCleanupRegistered => '在当前配置中被保存的 Runner 引用。';

  @override
  String get automationCleanupBusy => '用于当前配置中的连接、审查或未完成的工作。';

  @override
  String get automationCleanupChanged => '环境或当前选择发生变化。重新扫描并审查新计划。';

  @override
  String get automationCleanupExpired => '删除计划已过期, 请重新扫描并审查新计划。';

  @override
  String get automationCleanupUnsafe => '无法校验目录、收件、所有权或文件系统边界。删除不可用。';

  @override
  String get automationCleanupBudget => '清理扫描超过其文件系统或时限。无法删除; 解析限制后再次扫描。';

  @override
  String get automationCleanupCatalogUnavailable => '无法检查当前配置文件引用。在再次扫描前解锁并刷新 Runner 列表。';

  @override
  String get automationCleanupNonRoot => '使用拥有管理中的 Runner 目录的非root 账户。';

  @override
  String get automationCleanupFailed => '清理无法完成。在编制另一个删除计划之前再次扫描。';

  @override
  String get automationCleanupCurrentInterpreter => '运行此检查的口译人员使用; 无法删除。';

  @override
  String get automationInventoryExportAction => '导出清单...';

  @override
  String get automationInventoryExportTitle => '出口Ansible清单';

  @override
  String get automationInventoryExportDisclosure =>
      '只导出选定的主机地址、端口和SSH 用户名, 以及明确选定的Ansible 组。没有包含密码、密钥、证书引用或任意变量。只支持直接连接。';

  @override
  String get automationInventoryExportStaticHint =>
      '这是一个普通的静态文件,没有真实性或活的撤销保证. Outlook Ansible 需要独立配置的证书和 SSH 主机密钥信任; 文件授予了 ConsoleCrypt 访问权限。';

  @override
  String get automationInventoryExportHosts => '已保存的主机';

  @override
  String get automationInventoryExportHostsHint => '选择此项目的清单允许的 1- 32 主机。';

  @override
  String get automationInventoryExportGroups => 'Ansible个团体)';

  @override
  String get automationInventoryExportGroupsHint => '可选: 最多64个现有项目组。选择一个组从不添加主机。';

  @override
  String get automationInventoryExportNoHosts => '没有允许保存SSH 主机可用。';

  @override
  String get automationInventoryExportNoGroups => '该项目没有Ansible团体。';

  @override
  String automationInventoryExportSelected(int hosts, int groups) {
    return '选中 : $hosts 主机 —— $groups 组';
  }

  @override
  String get automationInventoryExportExternalOnly => '导出只用于外部目录。在导出前明确配置保存的主机。';

  @override
  String get automationInventoryExportChanged => '项目或保存的主机已更改。重新装入选择并导出。';

  @override
  String get automationInventoryExportSelectionInvalid => '仅选择当前项目目录允许的现有保存主机。';

  @override
  String get automationInventoryExportGroupsInvalid => '选中的Ansible 组对此导出不再有效。重新装入选中内容。';

  @override
  String get automationInventoryExportRouteUnsupported => '选定的主机使用跳转主机、代理机、隧道或其他不支持的选项SSH。此导出只支持直接连接。';

  @override
  String get automationInventoryExportEndpointUnsupported => '选定的主机地址或SSH用户名无法在静态盘点中安全表示.';

  @override
  String get automationInventoryExportTooLarge => '已发行的清单超过 256 KiB 导出限制。请选择更少的主机或组。';

  @override
  String get automationInventoryExportFailed => '无法导出目录。重新装入选择并再次尝试。';

  @override
  String get automationInventoryExportSave => '保存JSON...';

  @override
  String get automationInventoryExportSaved => '静态清单保存。';

  @override
  String get automationSetupBootstrapFailed => '内置的 Runner 设置无法启动。更新ConsoleCrypt 并再次尝试检测。现有环境没有改变。';

  @override
  String get automationSetupCommandTooLarge => 'Runner 设置命令超过了支持的大小。更新ConsoleCrypt 并再次尝试。';

  @override
  String get automationCleanupBootstrapFailed => '内置环境扫描仪无法启动。更新ConsoleCrypt 并再次扫描, 没有删除环境。';

  @override
  String get automationCleanupCommandTooLarge => '环境扫描命令超过支持的大小。没有删除环境。';

  @override
  String get automationProjectMenu => '项目';

  @override
  String get automationRunnerDeleteCandidate => '审查删除此环境';

  @override
  String get automationRunnerClearSelection => '清除环境选择';

  @override
  String get automationRunnerSelectionProtectedHint => '选定的环境受到保护。在审查删除前选择另一个环境或清除选择。保存的 Runner 引用仍然受到保护。';

  @override
  String get automationRunnerCandidateUnavailable => '此精确环境无法匹配到一个已验证的已管理候选人。将再次扫描, 没有选择要删除的其他环境。';

  @override
  String get noticesTitle => '通知';

  @override
  String get noticesSessionHelp => '此配置文件的任务和连接事件。此会话列出当保险库锁定或配置文件更改时清除。';

  @override
  String get noticesRefreshJobs => '刷新工作状态';

  @override
  String get noticesClear => '清除列表';

  @override
  String get noticesStreamUnavailable => '背景工作事件没有。刷新工作状态。';

  @override
  String get noticesRefreshNeeded => '有些事件可能不见了 刷新状态';

  @override
  String get noticesRefreshFailed => '无法刷新工作状态。';

  @override
  String get noticesInvalidResponse => '此工作目录无法安全读取。更新应用程序或检查配置。';

  @override
  String get noticesContextChanged => '配置文件或事件范围已更改。再次刷新。';

  @override
  String get noticesEmpty => '本届会议没有观察到重大事件。';

  @override
  String get noticesNew => '新设';

  @override
  String get noticesMarkRead => '标记为已读';

  @override
  String get noticesDismiss => '解雇通知';

  @override
  String get noticesDetails => '技术参考';

  @override
  String get noticesJobId => '任务代号';

  @override
  String get noticesSourceId => '源代码';

  @override
  String get noticesRevision => '订正';

  @override
  String get noticesCancelling => '取消';

  @override
  String noticesJobState(String state) {
    return '工作:$state';
  }

  @override
  String noticesObservedUtc(String time) {
    return '观察,协调世界时:$time';
  }

  @override
  String noticesUnread(String count) {
    return '未读通知:$count';
  }

  @override
  String get noticesPinReadFailure => '固定的观察: 刷新失败';

  @override
  String get noticesPinSourceChanged => '已停止 Pinned 观察: 来源已更改';

  @override
  String get noticesPinRetryLimit => '三次尝试失败后停止了平板观察';

  @override
  String get noticesPinPaused => '当应用程序暂停时, 已关闭的观察停止';

  @override
  String get noticesPinClosed => '应用程序关闭后, 关闭了。';

  @override
  String get noticesPinManual => '你停止了观察';

  @override
  String get noticesPinRecovered => '收回的固定观察';

  @override
  String noticesRuntimeState(String domain, String state) {
    return '$domain : $state';
  }

  @override
  String get noticesRuntimeOffline => '离线';

  @override
  String get noticesRuntimeError => '同步错误';

  @override
  String get noticesRuntimeStopped => '已停止';

  @override
  String get noticesRuntimeDeviceRevoked => '设备访问已取消';

  @override
  String get noticesRuntimeReauthRequired => '再次签名';

  @override
  String get noticesRuntimeAccountDisabled => '账户已禁用';

  @override
  String get noticesRuntimeNotTrusted => '需要设备批准';

  @override
  String get noticesRuntimeVaultGone => '无法同步保险库';

  @override
  String get noticesRuntimeUpgradeRequired => '需要客户更新';

  @override
  String get noticesRuntimeIdle => '没有活动同步操作';

  @override
  String get noticesRuntimeRunning => '隧道运行';

  @override
  String get noticesRuntimeFailed => '连接失败';

  @override
  String get noticesRuntimeConnected => '已连接';

  @override
  String get noticesRuntimeClosed => '届会闭幕';

  @override
  String get noticesRuntimeRecordId => '记录标识';

  @override
  String get noticesRuntimeUnsupported => '此后端不提供范围化的连接通知。任务通知仍然可用。';

  @override
  String get noticesRuntimeUnavailable => '连接事件流是关闭的。现有的观测是历史的; 重新启动ConsoleCrypt 以建立一个新的观察员。';

  @override
  String get noticesRuntimeIncomplete => '一些连接事件可能缺失。更新任务不会恢复其历史; 请检查相应的区域。';

  @override
  String get noticesRuntimeHelp => '仅观测到连接状态。打开区域不会重新连接、重新启动或重试任何东西。闭门会话不会证明命令成功。';

  @override
  String get automationFolderDirectHint => '链接主机文件夹以保持组同步,或复制其当前成员。子文件夹不在此列。';

  @override
  String get automationFolderSelect => 'ConsoleCrypt 主机文件夹';

  @override
  String get automationFolderMissing => '文件夹不可用';

  @override
  String get automationFolderUnavailable => '文件夹组成没有可用。选择一个有效的文件夹或取消链接。运行前需要重新审查。';

  @override
  String automationFolderMemberCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count SSH 本文件夹中的主机',
      one: '$count SSH 本文件夹的主机',
    );
    return '$_temp0';
  }

  @override
  String get automationFolderLink => '链接文件夹';

  @override
  String get automationFolderSnapshot => '复制当前成员';

  @override
  String get automationFolderUnlink => '取消链接文件夹';

  @override
  String get automationFolderChanged => '文件夹、项目或组成改变。重新装入组并再次审查映射。';

  @override
  String automationFolderLinkedName(String name) {
    return '链接文件夹 :$name';
  }

  @override
  String get automationDependencyTitle => '项目依赖关系';

  @override
  String get automationDependencyKind => '类型';

  @override
  String get automationDependencyRole => '作用';

  @override
  String get automationDependencyCollection => '收藏';

  @override
  String get automationDependencyName => '名称';

  @override
  String get automationDependencyVersion => '准确版本';

  @override
  String get automationDependencyUrl => '存档 HTTPS URL';

  @override
  String get automationDependencySha256 => '存档SHA-256';

  @override
  String get automationDependencyRunner => '运行者';

  @override
  String get automationDependencyReviewSource => '审查来源';

  @override
  String get automationDependencyDownload => '下载已审查的归档';

  @override
  String get automationDependencyReviewChanges => '审查变动';

  @override
  String get automationDependencyInstall => '安装依赖性';

  @override
  String get automationDependencyFirstOnly => '仅支持第一个依赖。添加到已安装的集中, 更新尚未可用。';

  @override
  String get automationDependencyManagedOnly => '目前只为受支助的管理项目安装,并保存了文件夹和Git项目。';

  @override
  String get automationDependencyProfileUnsupported => '此项目位置不支持安装。原项目文件被保存。';

  @override
  String get automationDependencyExcludedEntries => '此项目包含Git 控制项, 不支持此项目的安装。';

  @override
  String get automationDependencyPlatformUnsupported => '在此环境中不支持依赖性安装。';

  @override
  String get automationDependencyNetworkNotice => '正在下载已审查的连接HTTPS 源。校验和可以识别归档内容; 它没有建立出版商, 也没有使代码可信。';

  @override
  String get automationDependencyPreparing => '编制经核实的项目候选人';

  @override
  String get automationDependencyPublished => '安装在新项目修订版中。没有运行Playbook。';

  @override
  String get automationDependencyUncertain => '安装结果需要审查。不要重复安装。请检查当前项目状态。';

  @override
  String get automationDependencyCancelRequested => '请求取消';

  @override
  String get automationDependencyErrorInvalidInput => '请检查archive-date=中的日期值 (帮助) URL,名称,版本和SHA-256.';

  @override
  String get automationDependencyErrorChanged => '项目“ Runner” 或配置文件已更改。打开新的源代码审查。';

  @override
  String get automationDependencyErrorBusy => '另一项行动正在进行中。结束这次审查, 稍后再开始一项新的审查。';

  @override
  String get automationDependencyErrorUnavailable => '依赖性准备不可用。请检查项目和运行器。';

  @override
  String get automationPortableExportAction => '导出加密软件包...';

  @override
  String get automationPortableInspectAction => '检讨ConsoleCrypt 存档...';

  @override
  String get automationPortableExportTitle => '导出加密的项目包';

  @override
  String get automationPortableInspectTitle => '审查加密ConsoleCrypt 存档';

  @override
  String get automationPortablePassword => '单独的软件包密码';

  @override
  String get automationPortableConfirmPassword => '确认软件包密码';

  @override
  String get automationPortableWarning => '归档使用保存的项目文件。已知的秘密和控制文件不在此列。其他文件可能包含密码或私人数据。';

  @override
  String get automationPortableExcluded => '不包括在软件包中';

  @override
  String get automationPortableWholeProject => '整个项目';

  @override
  String get automationPortableChooseFiles => '选择文件';

  @override
  String get automationPortableSizeLimit => '归档可保存最多1000个文件和16个米B。每个文件最多必须为1个米B。要继续,选择更少或较小的文件。';

  @override
  String automationPortableContents(int files, String size) {
    return '文件 ${files}_$size';
  }

  @override
  String get automationPortableExport => '保存软件包';

  @override
  String get automationPortableInspect => '审查包';

  @override
  String get automationPortableSaved => '加密软件包已保存。项目文件没有更改。';

  @override
  String get automationPortableFailed => '软件包操作无法完成。';

  @override
  String get automationPortableInspectionOnly => '该软件包已经认证, 以供审查。尚未从软件包中创建项目; 没有导入项目。';

  @override
  String automationPortableSummary(int files, int bytes, int exclusions) {
    return '$files文件,$bytes字节,$exclusions排除';
  }

  @override
  String get automationPortablePreparing => '正在准备软件包审查...';

  @override
  String get automationPortableUnavailable => '此后端中没有项目包操作。';

  @override
  String get automationPortableInspectWarning => '使用此选项在导入前检查加密ConsoleCrypt 项目归档的内容。请输入导出时的归档密码。';

  @override
  String get automationPortableUseSaved => '使用已保存的文件';

  @override
  String get automationPortableImportTitle => '导入新项目';

  @override
  String get automationPortableImportWarning => '文件可能包含密码或私密数据。为新项目分别选择连接和目录/运行器设置。导入时不会运行文件。';

  @override
  String get automationPortableImportChooseFile => '选择 .ccproject';

  @override
  String get automationPortableImportNoFile => '未选择软件包';

  @override
  String get automationPortableImportName => '新建项目名称';

  @override
  String get automationPortableImportPrepare => '审查包';

  @override
  String get automationPortableImportAccept => '导入';

  @override
  String get automationPortableImportPreparing => '正在准备审查...';

  @override
  String get automationPortableImportAccepting => '创建已审查的项目...';

  @override
  String get automationPortableImportPreviewOnly => '新的项目只有在确认后才能创建.';

  @override
  String get automationPortableImportNormalizedName => '经审查的项目名称';

  @override
  String get automationPortableImportFutureId => '新建项目 ID';

  @override
  String get automationPortableImportLimitedReview => '此复核有时间限制, 在确认时会再次检查其有效性。';

  @override
  String get automationPortableImportInstalled => '创建新项目。';

  @override
  String get automationPortableImportUnavailable => '在此环境中无法导入。';

  @override
  String get automationPortableImportInvalidInput => '选择软件包,输入项目名称及其单独的密码。';

  @override
  String get automationPortableImportChanged => '原上下文已更改。关闭此窗口并再次打开导入。';

  @override
  String get automationPortableImportSourceChanged => '选中的软件包已更改。再次审查文件。';

  @override
  String get automationPortableImportUnsupported => '在此无法导入此软件包。';

  @override
  String get automationPortableImportBusy => '另一个导入是使用可用的槽。先关闭其审查。';

  @override
  String get automationPortableImportExpired => '这项审查不再有效,明确启动新的审查。';

  @override
  String get automationPortableImportResponse => '审查答复无法核实。';

  @override
  String get automationPortableImportFailed => '无法完成导入操作。';

  @override
  String get automationPortableImportRecoveryRequired => '导入结果不确定。在再次尝试之前检查项目。';

  @override
  String get automationPortableImportPlatformUnsupported => '在此平台上尚未支持从软件包创建项目。';

  @override
  String get automationPortableImportLocationUnavailable => '新项目没有受保护的地点。';

  @override
  String get automationPortableImportTreeUnsupported => '软件包结构无法导入。';

  @override
  String get automationPortableImportCapacityReached => '已经达到平行的进口限制。关闭其他审查。';

  @override
  String get noticesLegacyIntegrityWarning => '对象完整性验证失败';

  @override
  String get noticesLegacyServerRollback => '检测到服务器回滚';

  @override
  String get noticesLegacySecretConflict => '需要审查机密数据冲突';

  @override
  String get noticesLegacyConflictCopy => '创建了冲突副本';

  @override
  String get noticesLegacyDeviceRegistered => '新设备签名于';

  @override
  String get noticesLegacyApprovalRequested => '请求设备批准';

  @override
  String get noticesLegacyRecoveryChanged => '在另一个设备上更改回收设置';

  @override
  String get noticesLegacyKeyConflict => '报告了保险库钥匙冲突';

  @override
  String get noticesLegacyInvalidLog => '成员日志验证失败';

  @override
  String get noticesLegacyLogRollback => '检测到小组成员记录的回滚';

  @override
  String get noticesLegacyTeamRemoved => '进入小组的机会被取消';

  @override
  String get noticesLegacyEditConflict => '远程文件冲突需要审查';

  @override
  String get noticesLegacyEditRecovery => '未保存的工作副本可供使用';

  @override
  String get noticesLegacyEditSynced => '工作副本同步';

  @override
  String get noticesLegacyCompleted => '已完成';

  @override
  String get noticesLegacyEditError => '工作副本同步失败';

  @override
  String get noticesLegacyBackupFailed => '备份失败';

  @override
  String get noticesLegacyTransferFailed => '文件传输失败';

  @override
  String get automationVaultUse => '使用 Ansible 断层';

  @override
  String get automationVaultDisclosure => '明确选择保存的密码。连接只保存此项目中的引用。密码保存在ConsoleCrypt 中, 并且只传递到此已审查的任务。';

  @override
  String get automationVaultChoose => '选择默认密码';

  @override
  String get automationVaultAdd => '添加证书';

  @override
  String get automationVaultConnect => '连接到项目';

  @override
  String get automationVaultConnected => '选中默认密码; 在运行前验证项目。';

  @override
  String get automationTaskResults => '任务成果';

  @override
  String get automationTaskPageHelp => '仅在此日记页面中录入任务结果。缺少的值仍然未知。读取或重连接永远不会执行任务。';

  @override
  String get automationTaskUnnamed => '未命名的任务';

  @override
  String get automationTaskUnchanged => '未更改';

  @override
  String automationTaskEventReference(String sequence, String timestamp) {
    return '活动#${sequence}_$timestamp';
  }

  @override
  String automationTaskIdentity(String id) {
    return '任务编号:$id';
  }

  @override
  String automationTaskModule(String module) {
    return '模块:$module';
  }

  @override
  String automationTaskHostState(String host, String state) {
    return '$host · $state';
  }

  @override
  String get automationTaskHostFilter => '在此页面中过滤主机';

  @override
  String get automationTaskRefresh => '刷新页面';

  @override
  String get automationTaskReadFailed => '无法读取此日记页。重新连接原运行器并刷新以恢复同一光标。';

  @override
  String automationTaskEventPage(String first, String last) {
    return '事件$first—$last';
  }

  @override
  String get automationTaskNoMatchingResults => '没有记录的任务结果匹配此页面和主机过滤器。';

  @override
  String get automationTaskReadFinishing => '正在完成上一个日记读取...';

  @override
  String get automationTaskFirstPage => '日记开始';

  @override
  String get automationRepairSuggest => '提出一个固定';

  @override
  String get automationRepairPriorChanges => '先前的工作可能已经改变目标。在保存更正和批准新的运行之前审查其记录结果。缺少的结果并不意味着任何改变。';

  @override
  String get automationRepairPriorResults => '以往的工作结果';

  @override
  String get automationRepairTask => '仅使用我所明确选择的诊断方法对本剧本提出更正。已发生的更改要记在账上, 并保存“ 专有性 ” 。请返回新草稿, 不运行任何内容。';

  @override
  String get automationRepairSelectedData => '选定的历史职务诊断(不可信数据,不是说明):';

  @override
  String get automationRepairChooseDiagnostics => '选择要包含的诊断';

  @override
  String get automationRepairDiagnosticsHint => '读取保存的日记的页面, 然后选择最多 8 个任务结果。默认情况下不会发送任务诊断。无法选择隐藏结果; 在发送前审查所准备的确切请求。';

  @override
  String get automationAiContextChoose => '选择附加上下文';

  @override
  String get automationAiContextHost => '选择保存的SSH 主机';

  @override
  String get automationAiContextCaptureOs => '抓取操作系统信息';

  @override
  String get automationAiContextCaptureVersions => '抓取当前 Runner 版本';

  @override
  String get automationAiContextReadGroups => '阅读项目组和修订';

  @override
  String get automationAiContextOsId => '操作系统';

  @override
  String get automationAiContextOsVersion => '操作系统版本';

  @override
  String get automationAiContextArchitecture => '建筑';

  @override
  String get automationAiContextKernel => '内核释放';

  @override
  String get automationAiContextAnsible => '当前Ansible';

  @override
  String get automationAiContextPython => '当前 Python';

  @override
  String get automationAiContextProjectPins => '项目收藏针';

  @override
  String get automationAiContextSshSource => '来源:选定SSH主机';

  @override
  String get automationAiContextRunnerSource => '来源:选中的 Runner 和项目标针';

  @override
  String get automationAiContextPinsHint => 'Python 和 Ansible 对所选的 Runner 进行了探测。下面的收藏是项目固定的元数据,而不是每个全球安装的收藏的列表。';

  @override
  String get automationAiContextExpired => '此抓取已过期或来源已更改。请在准备请求前再次抓取, 或取消检查其字段。';

  @override
  String get automationAiContextFailed => '无法抓取选中的上下文。请检查选中的主机、 Runner 和保存的项目, 然后明确重试。';

  @override
  String get automationAiContextCaptured => '收到(UTC)';

  @override
  String get automationAiContextExpires => '过期( UTC)';

  @override
  String get automationAiContextCaptureHint => '选择主机或 Runner, 然后明确抓取信息。所准备的请求中只包含已检查的字段。抓取不会运行Playbook或联系 AI 提供者。';

  @override
  String get automationAiContextUnavailable => '此视图没有其它上下文。在解锁的配置文件中打开一个保存的项目, 然后重新开始此动作。';

  @override
  String get automationTargetSettings => '连接继承';

  @override
  String get automationTargetSettingsHint => '仅读此ConsoleCrypt 主机及其文件夹设置的预览。编辑打开现有的主机设置。实际的路径和访问权限将在最后运行审查时从选定的运行者中检查。';

  @override
  String get automationTargetSettingsProblem => '无法校验一些设置。审查主机和继承的设置; 最终校验可以拒绝未解决或未支持的路由。';

  @override
  String get automationTargetAlias => 'Ansible别名';

  @override
  String get automationReviewChanged => '前一次审查已无法使用。请检查当前输入, 在确认前再次验证。';

  @override
  String get automationReviewProjectRevision => '项目修订';

  @override
  String get automationReviewInventorySnapshot => '清单摘要';

  @override
  String get automationReviewRunnerId => '运行者 ID';

  @override
  String get automationReviewPrivileges => '执行账户和特权';

  @override
  String get automationReviewPrivilegesUnknown => '目标列表显示已审查的登录账户。播放簿、角色或命令所需的密钥升级没有被验证; 执行前审查项目。检查模式不会取消这些权限。';

  @override
  String get automationWslDiscoveryEmpty => '未发现注册的分布WSL.';

  @override
  String get automationWslDiscoveryFailed => '无法读取WSL 分布。在 Windows 中刷新列表或检查WSL。';

  @override
  String get automationWslDiscoveryHint =>
      '选择此 Runner 的分布。启用WSL, 并在 Windows 中单独安装分布, 然后刷新。可能需要管理员批准或重新启动; 运行Playbook绝不改变Windows 特性。';

  @override
  String get automationWslRefresh => '刷新WSL分发量';

  @override
  String get automationRunnerHelperVersion => '运行器帮助版本';

  @override
  String get automationRunnerProbeUnavailable => '安装返回了没有可显示的组件诊断。诊断保存的 Runner 检查当前环境。';

  @override
  String get automationSetupPythonMissing => '所选环境Linux没有可用的/usr/bin/python3. 配置支持的python与venv分开,或连接一个准备的环境,然后再次运行诊断.';

  @override
  String get automationReviewValuesPrivate => '数值和证书引用被隐藏。此列表描述了已编制的请求; 它没有显示可变的内容。';

  @override
  String get automationReviewNoVariables => '没有额外的变量';

  @override
  String automationReviewVariableCount(String count) {
    return '额外变量:$count';
  }

  @override
  String get automationReviewVariableNamePrivate => '私有变量名称';

  @override
  String get automationReviewValuePrivate => '隐藏值';

  @override
  String get automationReviewNotVerified => '未核实';

  @override
  String get automationReviewEscalation => '特权升级';

  @override
  String get automationReviewAccounts => '审查的登录账户';

  @override
  String get automationWslExecutableMissing => '在系统路径上找不到可执行文件WSL。';

  @override
  String get automationWslSpawnFailed => '无法启动可执行文件WSL。';

  @override
  String get automationWslTimedOut => 'WSL 发行发现超时.';

  @override
  String get automationWslReadFailed => '无法读取WSL 分发列表。';

  @override
  String get automationWslOutputTooLarge => 'WSL分发清单超过了安全尺寸限制.';

  @override
  String get automationWslNonzeroExit => 'WSL 发行发现未成功退出.';

  @override
  String get automationWslDiscoveryUnknown => 'WSL 发行发现状态未知。刷新以再次检查。';

  @override
  String get automationReviewDeclaredTitle => '宣布的升级设置';

  @override
  String get automationReviewDeclaredHelp => '这些是孤立验证报告的信息提示。有效的权限仍未验证 : 角色、包含和运行时间变量可能会改变它们。账户值被隐藏。';

  @override
  String automationReviewDeclaredSite(String play, String location) {
    return '游戏$play —— $location';
  }

  @override
  String automationReviewDeclaredFlags(String become, String user, String method) {
    return '成为:$become;成为_用户:$user;成为_方法:$method';
  }

  @override
  String get automationReviewDeclaredAbsent => '这里没有声明';

  @override
  String get automationReviewDeclaredEnabled => '已宣布为';

  @override
  String get automationReviewDeclaredDisabled => '已宣布终止';

  @override
  String get automationReviewDeclaredDynamic => '动态; 未解决';

  @override
  String get automationReviewDeclaredTruncated => '声明列表不完整,因为达到了扫描限制.';

  @override
  String get automationReviewDeclaredEmpty => '在扫描节点中没有发现明显的升级指令,有效权限仍未核实。';

  @override
  String get automationOwnerConfirmationTitle => '其他生产核准';

  @override
  String get automationOwnerConfirmationHint => '使用您的保险库密码确认此已审查的工作。这是保险库所有人的额外协议, 而不是第二人批准。';

  @override
  String get automationOwnerManifestDigest => '经审查的表列指纹';

  @override
  String get automationOwnerConfirmAndRun => '确认并运行';

  @override
  String get automationOwnerConfirmationUnavailable => '本次审查没有额外的批准。更新运行者并再次检查项目。';

  @override
  String get automationOwnerReviewConfirm => '批准生产运行';

  @override
  String get automationWslPreparationTitle => '准备WSL';

  @override
  String get automationWslPreparationNotice => 'Windows 特性和发行安装可能需要管理员许可, 下载和Windows 重新启动。这里没有自动运行的功能。在重新启动前保存您的工作。';

  @override
  String get automationWslPreparationInspectTitle => '1. 检查已安装的分布';

  @override
  String get automationWslPreparationInspect =>
      '运行于PowerShell 中。空列表或发现错误不能证明Windows 特性已禁用。如果一个合适的WSL2 分布符已经存在, 请保留它, 并用步骤 3 继续。';

  @override
  String get automationWslPreparationInstallTitle => '2. 必要时安装Ubuntu 24.04';

  @override
  String get automationWslPreparationInstall =>
      '只有当您需要一个新的分发: 作为管理员打开PowerShell 并运行此命令时, 才会按下Windows 提示并重新启动您自己。现有的分发不被ConsoleCrypt 所取代。';

  @override
  String get automationWslPreparationUserTitle => '3. 编制Linux账户';

  @override
  String get automationWslPreparationUser =>
      '从Windows启动菜单打开已安装的分发. 完成首发设置,并创建一个带有密码的普通Linux用户. ConsoleCrypt不请求或保存sudo密码.';

  @override
  String get automationWslPreparationRunnerTitle => '4. 返回和检查环境';

  @override
  String get automationWslPreparationRunner =>
      '关闭此指南, 刷新分发列表, 明确选择您的分发和查找环境。丢失的软件包会单独显示。在环境中使用运行来安装先决条件, 然后重复检测并明确安装或选择 Runner。';

  @override
  String get automationServiceConnectionMode => 'Linux次)服务(排定运行)';

  @override
  String get automationServiceConnectionIdentity => '安装服务身份UUID';

  @override
  String get automationServiceConnectionHint =>
      '使用它保存的SSH主机和服务UUID连接一个已安装的ConsoleCryptrunner服务. 一旦授权,服务可以在此应用程序关闭时运行调度表.';

  @override
  String get automationServiceConnectionManageHint => '准备一个预定的代表团,以便连接、检查或取消访问。';

  @override
  String get automationServiceDelegate => '指定一个时间表';

  @override
  String get automationServiceUseDelegation => '此运行器是一个服务。请使用单独的进度审查。';

  @override
  String get automationServiceUnavailable => '本届会议没有服务代表团。';

  @override
  String get automationServiceScheduleTitle => '服务时间表';

  @override
  String get automationServiceContinuationDisclosure =>
      '在确认后, 当对话框关闭或保险库被锁定时, 服务可能会继续, 直至批准到期。离线吊销可能延迟到那时。生产目标和Ansible 断层输入无法用于此目的。';

  @override
  String get automationServiceRunSelectionRequired => '为新日程选择一个播放簿、目标和有效的运行选项。服务状态和撤销是分开提供的。';

  @override
  String get automationServiceZone => '时间表时区';

  @override
  String get automationServiceHour => '小时';

  @override
  String get automationServiceMinute => '分钟';

  @override
  String get automationServiceWeekdays => '每周天数';

  @override
  String get automationServiceValidity => '权限窗口';

  @override
  String automationServiceValidityHours(String hours) {
    return '$hours小时)';
  }

  @override
  String get automationServiceMaxJobs => '最大工作总数';

  @override
  String get automationServiceMaxDaily => '世界协调时每一天的最大工作';

  @override
  String get automationServicePreview => '审查代表团';

  @override
  String get automationServiceConfirm => '确认代表团';

  @override
  String get automationServiceExpiry => '许可过期( UTC)';

  @override
  String automationServiceConsentWindow(String seconds) {
    return '在$seconds 秒) 内确认。在确认时再次检查过期。';
  }

  @override
  String get automationServiceCredentials => '选中的加密证书胶囊';

  @override
  String get automationServiceKeys => '服务身份和钥匙指纹';

  @override
  String get automationServiceIdentity => '现有服务身份';

  @override
  String get automationServiceOrigin => '执行Linux';

  @override
  String get automationServiceUnknown => '交付不确定。不要重试确认。在采取其他行动前读取服务状态。';

  @override
  String get automationServiceAcknowledged => '服务处承认了这个代表团,但这并不意味着正在工作。';

  @override
  String get automationServiceBlocked => '本地屏蔽。远程吊销未确认; 离线服务可持续到到期。';

  @override
  String get automationServiceStatus => '已观测服务状态';

  @override
  String get automationServiceReadStatus => '读取状态';

  @override
  String get automationServiceRevoke => '撤销权限';

  @override
  String get automationServiceConfirmRevoke => '确认撤销';

  @override
  String automationServiceRevokeReview(String runner) {
    return '取消$runner的许可 ? 远程工作可能具有部分效果; 离线撤销不是立即的.';
  }

  @override
  String get automationServiceDataOnly => '状态是一种观察,而不是允许运行或证明完成.';

  @override
  String get automationServiceChanged => '准备的上下文不可用或更改, 请再次明确审查。';

  @override
  String get automationServiceReviewExpired => '确认窗口过期 准备重新审核';

  @override
  String get automationServiceUnbound => '随函附上无代表团名单。';

  @override
  String get automationServicePending => '尚未进行设置审查';

  @override
  String get automationServiceEnrolled => '代表团由服务处录制';

  @override
  String get automationServiceRestored => '服务恢复了记录';

  @override
  String get automationServiceRevocationRecorded => '该处记录了退职情况;这不能证明飞行中的目标任务已经停止。';

  @override
  String get automationServiceRemoteUnknown => '远程服务状态未知';

  @override
  String get automationServiceScheduleSemantics => '一次一个任务。 Grace: 60 秒。本地时间和空档被跳过; 重复本地时间使用第一次发生。';

  @override
  String get automationOutputRetired => '来文方报告说,在保留期之后,详细产出被删除。';

  @override
  String get automationWslWizardReadOnly => '只读诊断。没有安装、提升、重新启动或默认。';

  @override
  String get automationWslWizardWindows => '1. 目前的意见Windows';

  @override
  String automationWslWizardSubsystem(String state) {
    return 'Windows 子系统用于Linux: $state';
  }

  @override
  String automationWslWizardVirtualMachine(String state) {
    return '虚拟机平台:$state';
  }

  @override
  String get automationWslWizardFeatureEnabled => '已启用';

  @override
  String get automationWslWizardFeatureDisabled => '已禁用';

  @override
  String get automationWslWizardFeatureEnablePending => '启用待决';

  @override
  String get automationWslWizardFeatureDisablePending => '禁用待办事宜';

  @override
  String get automationWslWizardElevationRequired => '未知: 读取此特性需要提升进程';

  @override
  String get automationWslWizardElevated => '这一过程得到了提升。检查仍然只读。';

  @override
  String get automationWslWizardNotElevated => '此进程没有提升。特性状态未知。';

  @override
  String get automationWslWizardElevationUnknown => '无法观测到进程高度。';

  @override
  String get automationWslWizardRebootPresent => 'Windows 报告了待恢复的指标,它们没有指明每个重新开始的要求。';

  @override
  String get automationWslWizardRebootNotObserved => '在已检查的登记册位置没有看到任何指标。这并不能保证重新开始是不必要的。';

  @override
  String get automationWslWizardRebootUnknown => '重新启动指标无法完全读取。';

  @override
  String automationWslWizardCaptured(String time) {
    return 'Windows只读快照,捕获于$time (UTC).';
  }

  @override
  String get automationWslWizardUnavailable => 'Windows 本次原始会议无法进行特征检查,仍可提供手册说明。';

  @override
  String get automationWslWizardInspect => '检查Windows(只读)';

  @override
  String get automationWslWizardEnvironment => '2. 选择和检查现有的分销';

  @override
  String get automationWslWizardDiscoveryUnknown => '发现系统尚未建立WSL 分布, 重新检查。';

  @override
  String get automationWslWizardInspectLinux => '检查选中的环境( 只读)';

  @override
  String get automationWslWizardLinuxReport => '观察到的环境事实';

  @override
  String automationWslWizardMissing(String name) {
    return '缺少先决条件:$name';
  }

  @override
  String get automationWslWizardNoPrepared => '没有报告当前准备的运行器环境。';

  @override
  String get automationWslWizardSelectPrepared => '选择可观察到的准备环境';

  @override
  String get automationWslWizardUsePrepared => '使用此环境的形式';

  @override
  String get automationWslWizardPreparedNotice => '这只能填充 Runner 表格。保存和连接仍然是单独的明确动作。';

  @override
  String get automationWslWizardReviewPlan => '审查手册编制计划';

  @override
  String get automationWslWizardPlan => '3. 手工准备和返回';

  @override
  String get automationWslWizardPlanUnknown => '有些观察Windows是未知的。在任何人工行政工作之后,重新进行只读检查;未知并不意味着禁用。';

  @override
  String get automationWslWizardPlanKnown => '使用上面观察到的特性状态来选择相应的手动步骤。这里不执行更改。';

  @override
  String get automationWslWizardPlanManual =>
      '如果组件被禁用, 请使用下面的明示Windows 安装指令。如果待过渡, 只需在Windows 请求时完成Windows 操作并重新启动。请保留一个已准备好的分发; 不更改默认值。';

  @override
  String get automationWslWizardTerminalNotice => '在不发送命令的情况下选择要打开的 shell。只执行您审查的手动步骤。';

  @override
  String get automationWslWizardChooseTerminal => '选择现有的本地终端';

  @override
  String get automationWslWizardOpenTerminal => '打开选定的终端';

  @override
  String get automationWslWizardRecheck => '经过人工准备后,返回这里并重复Windows检查,分配发现和选定的环境检查.';

  @override
  String get automationWslWizardFailed => '检查或移交没有完成,无法推断是否准备就绪。';

  @override
  String get localAiStopUnconfirmed => '本地模式进程退出尚未确认。启动另一个模式被屏蔽。重试停止; 模式数据可能一直保存到退出为止。';

  @override
  String get automationServiceResultsRead => '读取记录的服务结果';

  @override
  String get automationServiceResultsUnavailable => '无法获取记录的服务结果, 或更改其上下文。没有装入新结果。重新明确读取以重试。';

  @override
  String get automationServiceResultsRecorded => '已录制的终端结果来自选中的服务。这不能证明当前的目标状态, 也不能证明所有更改都已完成。';

  @override
  String get automationServiceResultsEmpty => '没有记录匹配的终端任务。';

  @override
  String get automationServiceResultsPartial => '目标结果未知; 变化可能是局部的。下面的运行器结果无法证明任务成功。';

  @override
  String get automationServiceResultsRunner => '已录制的运行者结果';

  @override
  String get automationServiceResultsTarget => '目标成果';

  @override
  String get automationServiceResultsKnown => '已录制;参见工作结果';

  @override
  String get automationServiceResultsDue => '预定时间( UTC)';

  @override
  String get automationServiceResultsUpdated => '记录结果(UTC)';

  @override
  String get automationServiceResultsCancelled => '请求取消';

  @override
  String get automationServiceResultsTimedOut => '记录超时';

  @override
  String get automationResultDetailsFiltered => '此视图显示所支持的已记录元数据。 Raw 或额外字段在此被扣留; 存储的任务结果不变。';

  @override
  String get automationResultDetailsUnknown => '本记录中没有支持的详细控制证据。';

  @override
  String get automationAuditTitle => '录取审计';

  @override
  String get automationAuditDisclosure => '历史的录取元数据不允许访问. 路由摘要描述了批准的路线;这种观点没有记录隧道活动,也没有声称有不可改变的日志.';

  @override
  String get automationAuditUnavailable => '审计元数据没有可用,不匹配此任务或超过支持的界限。';

  @override
  String get automationAuditLegacy => '这个遗留的工作没有记录启动- 设备元数据。';

  @override
  String get automationAuditRunnerUnrecorded => '运行者没有记录此工作的录取元数据。';

  @override
  String get automationAuditNotObserved => '运行器录入元数据未观察到。';

  @override
  String get automationAuditInitiatingDevice => '启动签名设备';

  @override
  String get automationAuditJobId => '任务代号';

  @override
  String get automationAuditProjectId => '项目标识';

  @override
  String get automationAuditRunnerId => '运行者 ID';

  @override
  String get automationAuditRevision => '核定修订摘要';

  @override
  String get automationAuditManifest => '说明摘要';

  @override
  String get automationAuditPurpose => '录音操作目的';

  @override
  String get automationAuditIssuedUtc => '已签明单发行时间(UTC)';

  @override
  String get automationAuditOwnerEvidence => '额外的所有者确认摘要';

  @override
  String automationAuditTargets(String count) {
    return '准确记录的目标:$count';
  }

  @override
  String get automationAuditTargetId => '核定主机代号';

  @override
  String get automationAuditRouteDigest => '核定路线摘要';

  @override
  String get automationDeploymentTitle => 'SSH访问配置文件';

  @override
  String get automationDeploymentHint => '当项目需要不同的证书时使用配置文件SSH。否则它会使用保存在每个主机上的证书。';

  @override
  String get automationDeploymentUnavailable => '原始项目证书上下文不可用或更改。在进行新选择之前关闭并重新打开设置或运行格式。';

  @override
  String get automationDeploymentLegacy => '使用每个主机保存的SSH证书.';

  @override
  String get automationDeploymentChoose => '选择访问配置';

  @override
  String get automationDeploymentNew => '新简介';

  @override
  String get automationDeploymentAdd => '添加配置文件';

  @override
  String get automationDeploymentProduction => '生产主机需要所有者确认';

  @override
  String get automationDeploymentProductionHint => '标注为生产的主机总是需要所有者确认.';

  @override
  String get automationDeploymentAddReference => '添加主机和证书';

  @override
  String get automationDeploymentReferenceMissing => '无全权证书';

  @override
  String get automationDeploymentReuse => '为每个主机选择保存的证书, 包括任何跳转到主机。';

  @override
  String get automationDeploymentRemoveTitle => '使用主机卡的证书';

  @override
  String get automationDeploymentRemoveHint => '保存后,项目会使用每个主机上保存的证书.';

  @override
  String get automationDeploymentFrozenReferences => '原生冷冻主机 – 证书参考';

  @override
  String get automationDeploymentRefresh => '重新装入配置文件';

  @override
  String get automationDeploymentErrorRequired => '在准备此项目之前明确选择一个部署环境。';

  @override
  String get automationDeploymentErrorChanged => '原始项目或证书选择更改了。重新打开运行的窗体或项目设置, 读取当前上下文, 再审查选择。';

  @override
  String get automationDeploymentErrorMissing => '在此环境中为每个实际目标添加现有的证书引用并跳转到主机。然后准备新的审查; 全球主机证书不会自动替换。';

  @override
  String get automationDeploymentErrorShared => '共享主机或证书覆盖在此流程中不支持。使用原始的发行/ 共享工作流程, 或者明确选择支持的个人主机和证书。';

  @override
  String get automationDeploymentErrorSchema => '此项目证书策略有一个不支持的策略, 并且只读。保持不变, 用兼容的客户端打开它。';

  @override
  String get automationDeploymentErrorBusy => '项目准备正在进行中。让它完成或明确取消审查, 然后重新打开证书设置。不进行自动重试。';

  @override
  String get automationMetadataTitle => '项目设置同步';

  @override
  String get automationSettingsSyncSetup => '设置同步';

  @override
  String get automationSettingsSyncEnable => '启用同步';

  @override
  String automationSettingsSyncConfirmHint(String version) {
    return '启用此账户中的个人保险库的项目设置同步。设备需要ConsoleCrypt$version, 或者以后需要保持同步。';
  }

  @override
  String get automationSettingsSyncRefreshHint => '同步启用。刷新目录以从其他设备获取设置。';

  @override
  String get automationSettingsSyncUpdateDevices => '更新ConsoleCrypt 这个账户的设备, 然后再次打开同步设置。';

  @override
  String get automationSettingsSyncUnavailable => '服务器无法确认设置同步。请检查它是否支持设置同步, 您是否作为个人保险库的所有人签名。';

  @override
  String get automationSettingsSyncEnabled => '同步已准备好。您可以将项目设置保存到您的账户。';

  @override
  String get automationMetadataHint => '将设置和引用与项目文件分开同步。导入的引用无法运行Playbook。';

  @override
  String get automationMetadataUnknown => '保存未确认。刷新以检查当前目录后再尝试。';

  @override
  String get automationMetadataPublished => '已保存到加密同步目录的项目设置。';

  @override
  String get automationMetadataLocal => '这是本地配置文件。使用同步配置文件在设备之间共享项目设置。';

  @override
  String get automationMetadataCompatibility => '设置同步需要每个设备的兼容版本。此目录目前只读。';

  @override
  String get automationMetadataReadOnly => '此目录由更新的版本创建. Update ConsoleCrypt to change it.';

  @override
  String get automationMetadataConflict => '项目设置有同步冲突。在发布更改前解决它。';

  @override
  String get automationMetadataChanged => '项目或配置文件已更改。再次打开其设置。';

  @override
  String get automationMetadataUnavailable => '无法读取项目设置。刷新以再次尝试。';

  @override
  String get automationMetadataEmpty => '未同步项目引用。';

  @override
  String get automationMetadataRevision => '订正';

  @override
  String get automationMetadataFiles => '文件';

  @override
  String get automationMetadataUnresolved => '无法获取的参考文献';

  @override
  String get automationMetadataRecordHint => '只设置和引用。单独打开项目文件以运行Playbook。';

  @override
  String get automationMetadataRefresh => '刷新目录';

  @override
  String get automationMetadataPublish => '同步项目设置';

  @override
  String get automationWorkflowHint => '选择一个Playbook和主机或一个组,审查并运行. Ansible 运行在选定的运行者中。';

  @override
  String get automationVaultFileTitle => 'Ansible号)';

  @override
  String get automationVaultEncrypt => '加密YAML';

  @override
  String get automationVaultDecrypt => '解密YAML';

  @override
  String get automationVaultPasswordSource => '密码来源';

  @override
  String get automationVaultManual => '输入密码';

  @override
  String get automationVaultSavedPassword => '已保存密码';

  @override
  String get automationVaultManualRunHint => '只有这次运行';

  @override
  String get automationVaultManualLinked => '此项目已经链接了保存的“ 默认” 密码。选择此项目的保存密码。';

  @override
  String get automationVaultProjectFile => '.vault/密码项目';

  @override
  String get automationVaultPassword => 'Ansible 默认密码';

  @override
  String get automationVaultFileHint => '只更改了选中的保存YAML。项目密码会留在本地, 并被排除在运行快照之外。';

  @override
  String get automationVaultDecryptHint => '解密YAML将保存为纯文本.';

  @override
  String get automationTreeRoot => '项目根';

  @override
  String get automationTreeEmpty => '此项目中没有文件';

  @override
  String get automationRunPasswords => '运行密码';

  @override
  String get automationRunPasswordsHint => '仅用于本次运行。密码不会保存在项目、日志或运行参数中。';

  @override
  String get automationUseBecomePassword => '启动需要 sudo / 成为密码';

  @override
  String get automationBecomePassword => 'sudo / 成为密码';

  @override
  String get automationRunPasswordsConfigured => '已为本次运行设置密码。';

  @override
  String get automationRunPasswordsUnavailable => '当前无法使用运行密码。请重新打开运行表单或选择常规 Runner。';

  @override
  String get automationLibrary => '资料库';

  @override
  String get automationLibrarySearch => '找到一个项目';

  @override
  String get automationLibraryHint => '选择 Playbook 或角色并进行编辑，也可以使用 AI 草拟更改。运行前请检查所选主机和任务内容。';

  @override
  String get automationLibraryNoMatches => '没有匹配的项目';

  @override
  String get automationLibraryPlaybooks => 'Playbook';

  @override
  String get automationLibraryRoles => '角色';

  @override
  String get automationLibraryFiles => '文件';

  @override
  String get automationLibraryNoPlaybooks => '此项目中没有Playbook。创建Playbook或导入一个项目。';

  @override
  String get automationLibraryNoRoles => '在此项目中没有角色。创建角色或添加已核实的依赖。';

  @override
  String get automationLibraryRoleHint => '为编辑打开角色任务。通过Playbook运行角色。';

  @override
  String get automationPortableImportArchiveFormat => '选择加密的ConsoleCrypt项目档案.ccproject.';

  @override
  String get automationPortableImportArchiveSize => '归档超过项目导入的大小限制。';

  @override
  String get automationPortableImportArchiveRead => '无法读取选中的归档。将其下载到此设备并再次尝试。';

  @override
  String get keepassDeviceKeyInvalidated => '此设备的密钥已失效。请用主密码打开数据库，再启用生物识别解锁。';

  @override
  String get keepassDeviceAuthFailed => '系统身份验证失败。请重试，或使用主密码打开数据库。';

  @override
  String get automationLibraryExportVersion => '导出版本';

  @override
  String get automationLibraryImportArchive => '导入项目归档';

  @override
  String get automationImportPlaybook => '打开Playbook文件';

  @override
  String get automationImportPlaybookInvalid => '选择 a UTF-8 Playbook文件 (.yml 或.yaml) 最多可达 256 KB.';

  @override
  String get automationLibraryExchangeHint => '导出当前项目快照, 并将归档传递给队友。导入后将创建一个单独的项目。';

  @override
  String get automationVersionsTitle => '版本';

  @override
  String get automationVersionsHint => '保存当前项目快照。打开保存的版本会创建单独的项目并保存此项目。';

  @override
  String get automationVersionsName => '版本名称';

  @override
  String get automationVersionsSave => '保存版本';

  @override
  String get automationVersionsOpen => '作为新项目打开';

  @override
  String get automationVersionsProjectName => '新建项目名称';

  @override
  String get automationVersionsEmpty => '尚未保存版本';

  @override
  String get automationVersionsUnavailable => '无法读取或保存此版本。刷新项目并再次尝试。';

  @override
  String get automationSchedulesTitle => '计划任务';

  @override
  String get automationSchedulesIntro => '即使 ConsoleCrypt 已关闭，也可按计划运行 Playbook。批准后 24 小时内，服务器上的 Runner 可以执行该任务。';

  @override
  String get automationSchedulesRunnerRequired => '在“设置 → Runner 环境”中添加服务器 Runner 后即可安排运行。';

  @override
  String get automationSchedulesConfigure => '设定时间和天数';

  @override
  String get automationSchedulesNew => '新建计划任务';

  @override
  String get automationSchedulesUnavailable => '无法装入进度表。请尝试刷新。';

  @override
  String get automationSchedulesEmpty => '暂无计划任务。';

  @override
  String get automationSchedulesDetailsUnavailable => '时间表细节不详。';

  @override
  String get automationSchedulesExpiry => '权限过期';

  @override
  String get automationSchedulesExpired => '权限过期';

  @override
  String get automationSchedulesBlocked => '此设备被屏蔽';

  @override
  String get automationSchedulesAcknowledged => '服务器接受';

  @override
  String get automationSchedulesPending => '等待设置';

  @override
  String get automationSchedulesUnknown => '交付未经证实';

  @override
  String get automationRemoteRunnerPurpose => 'ConsoleCrypt 启动这个服务器上的运行器 结束SSH 当你从应用程序启动一个任务。';

  @override
  String get automationGitHttpHint => 'HTTP下载没有加密。当可用时使用HTTPS,并在运行任务前审查导入的文件。';

  @override
  String get automationFileCopyPath => '复制相对路径';

  @override
  String automationFileDeleteConfirm(String path) {
    return '从磁盘中删除项目文件$path ?';
  }

  @override
  String automationDirectoryDeleteConfirm(String path) {
    return '删除空文件夹$path ?';
  }

  @override
  String get automationProjectCopyFolder => '复制文件夹路径';

  @override
  String get automationProjectRemove => '从库中删除';

  @override
  String automationProjectRemoveConfirm(String name) {
    return '从库中删除$name ? 它的文件会留在磁盘上。';
  }

  @override
  String get automationDeploymentDeleteProfile => '删除配置文件';

  @override
  String get automationGroupAdd => '添加组';

  @override
  String get automationGroupEmpty => '还没有团体';

  @override
  String get automationGroupChoose => '选择组合';

  @override
  String automationGroupUse(String group) {
    return '在 Playbook 中使用 hosts: $group';
  }

  @override
  String get automationGroupUnlinkToEdit => '在重命名或删除此组之前先取消文件夹链接。';

  @override
  String get automationDirectoryNewFile => '新建文件...';

  @override
  String get keepassChooseEntryIcon => '选择图标';

  @override
  String keepassEntryIconName(String icon) {
    String _temp0 = intl.Intl.selectLogic(icon, {
      'Key': '默认',
      'LockOpen': '锁定',
      'Identity': '身份',
      'World': '网站',
      'EMail': '电子邮件',
      'NetworkServer': '服务器',
      'Monitor': '计算机',
      'Console': '终端',
      'Homebanking': '银行',
      'Certificate': '证书',
      'Book': '书',
      'Star': '星号',
      'Folder': '文件夹',
      'other': '图标',
    });
    return '$_temp0';
  }

  @override
  String get terminalOnlyPalette => '命令面板';

  @override
  String get terminalOnlyNewWindow => '打开新窗口';

  @override
  String get terminalOnlyOpen => '只打开终端';

  @override
  String get terminalOnlyWindowError => '无法打开新终端窗口。';

  @override
  String get terminalOnlyShellError => '无法打开本地 shell。';

  @override
  String get terminalOnlyShellExited => '炮弹已经离开';

  @override
  String get terminalLocalShellSearch => '搜索本地贝壳';

  @override
  String get terminalLocalShellNoMatch => '没有匹配的贝壳';

  @override
  String get navigationDragHint => '拖曳区域来重新排序它们。使用复选框来显示或隐藏它们。';

  @override
  String get keepassNerdIconSearch => '按名称搜索图标';

  @override
  String get keepassNerdIconTitle => '神经字体图标';

  @override
  String get keepassNerdIconLoadFailed => '无法装入图标';

  @override
  String get dockerKubeDeploy => '部署到 Kubernetes · 实验功能';

  @override
  String get dockerKubeIntro => '一个 Linux 副本和一个内部 ClusterIP Service。请单独配置 Docker 环境覆盖项与 Compose 依赖。';

  @override
  String get dockerKubeUnsupported => '向导需要配置已经验证的 Linux 容器，且容器无需使用宿主机网络、设备或额外权限即可运行。请选择兼容的容器，或手动准备 Kubernetes 清单。';

  @override
  String get dockerKubeMountsBlocked => '此容器挂载了文件或卷。向导适用于没有挂载的容器。请选择其他容器，或手动准备 Kubernetes 清单并配置挂载。';

  @override
  String get dockerKubeImage => '已发布的镜像（registry/repository:tag 或 @sha256:digest）';

  @override
  String get dockerKubeManifestYaml => '部署清单 (YAML)';

  @override
  String get dockerKubeNoProvider => '请在设置中添加 AI 提供商以生成清单。';

  @override
  String get dockerKubeImageHelp => '使用集群节点可拉取的标签或摘要。私有镜像可能需要 imagePullSecret。';

  @override
  String get dockerKubePort => '容器和服务的 TCP 端口';

  @override
  String get dockerKubePullSecret => '此命名空间中已有的 imagePullSecret（可选）';

  @override
  String get dockerKubeNewNamespace => '新命名空间';

  @override
  String get dockerKubeCreateNamespace => '立即创建命名空间';

  @override
  String get dockerKubeNamespaceHelp => '创建命名空间会立即修改集群。命名空间创建后，可以在服务器上试运行应用。';

  @override
  String get dockerKubeNamespaceCreated => '命名空间已创建。取消操作后仍会保留。';

  @override
  String get dockerKubeGenerate => '使用 AI 生成 YAML';

  @override
  String get dockerKubeTemplate => '使用模板';

  @override
  String get dockerKubePrompt => '提示词';

  @override
  String get dockerKubeAiReview => '发送前可以编辑提示词或补充要求。生成后，ConsoleCrypt 会在本地添加启动命令。';

  @override
  String get dockerKubeSendAi => '发送提示词';

  @override
  String get dockerKubeManifestRejected =>
      '清单必须包含且仅包含一个 Deployment 和一个 ClusterIP 服务，并使用所选镜像、名称及端口。只能修改资源配置和 TCP 就绪探测的时间参数。';

  @override
  String get dockerKubeInvalidTarget => '请检查应用和命名空间的名称、已发布镜像的仓库及标签或摘要、TCP 端口和镜像拉取密钥的名称。';

  @override
  String get dockerKubeAcknowledge => '已核对镜像、平台和启动命令';

  @override
  String get dockerKubeValidated => '两个资源均已通过服务器端试运行。部署只会创建新资源，保留已有资源。';

  @override
  String get dockerKubeFailed => '操作失败。请检查集群访问权限、写入权限、资源名称冲突和清单验证结果。下方列出的已创建资源会保留在集群中。';

  @override
  String get dockerKubeCreated => '已创建的资源';

  @override
  String get dockerKubeReady => 'Deployment 已就绪';

  @override
  String get dockerKubePending => '正在等待 Deployment 的副本就绪。';

  @override
  String get dockerKubeCheckStatus => '检查就绪状态';

  @override
  String get dockerKubeUnavailable => '已保存的 Kubernetes 上下文或所需的原生 API 不可用。';

  @override
  String get dockerKubeAiFailed => 'AI 未能生成完整响应。可以使用模板。';

  @override
  String get dockerKubeContext => 'Kubernetes 上下文';

  @override
  String get dbLocalEngineNewConnectionHelp => '此已保存的连接可选择“此电脑”。若要绑定到特定的本地引擎，请新建连接。';

  @override
  String get composeAiGenerate => '使用 AI 生成 Compose';

  @override
  String get composeAiContextHelp => '请描述所需的服务。仅发送此描述，当前 YAML、容器环境和密钥留在本机。';

  @override
  String get composeAiDescription => 'Compose 项目需要运行什么？';

  @override
  String get composeAiReview => '请检查生成的 YAML。插入后会打开计划，启动服务需要单独操作。';

  @override
  String get composeAiUse => '插入编辑器';

  @override
  String get composeAiInvalid => 'AI 未返回有效的 Compose YAML。请调整描述后重试。';

  @override
  String get composeAiReplaceTitle => '替换 YAML？';

  @override
  String get composeAiReplaceHelp => '编辑器已有内容。要用 AI 的建议替换吗？';

  @override
  String dockerPortCount(int total) {
    return '$total 个端口';
  }

  @override
  String dockerPortRange(int start, int end, int total) {
    return '第 $start–$end 项，共 $total 项';
  }

  @override
  String get dockerKubeStartupUnavailable => 'Docker 返回的启动命令不完整或已被遮盖。请检查容器的启动命令并手动编写清单。';

  @override
  String get dockerKubeNameTaken => '此命名空间中已存在同名资源。请选择其他名称，或在 Kubernetes 中打开现有资源。';

  @override
  String get dockerKubeTerminated => '容器进程已退出。请检查启动命令。';

  @override
  String dockerKubeExitCode(int code) {
    return '退出码 $code';
  }

  @override
  String get dockerKubeUnschedulable => 'Kubernetes 无法将 Pod 调度到节点。';

  @override
  String get dockerKubeContainerConfig => 'Kubernetes 无法配置或启动容器。请查看 Pod 事件。';

  @override
  String get dockerKubeDeadlineExceeded => 'Deployment 已超过部署进度期限。';

  @override
  String get dockerKubeReplicaFailure => 'Deployment 控制器报告副本失败。请查看 ReplicaSet 事件。';

  @override
  String get dockerKubeStatusUnavailable => '无法读取已创建的 Deployment 的状态。请检查集群连接和读取权限。';

  @override
  String get dockerKubePodDetailsUnavailable => '无法获取 Pod 的诊断详情。';

  @override
  String get dockerKubeReplaced => '此名称现已属于另一个 Deployment。请在 Kubernetes 中打开该资源以查看状态。';

  @override
  String tunnelsLocalAddressUnavailable(String address) {
    return '无法监听 $address。请检查此设备上的端口是否已被占用或保留。';
  }

  @override
  String get externalVaultConnections => '已保存的连接';

  @override
  String get externalVaultNoConnections => '暂无已保存的连接';

  @override
  String get wslcRuntimeUnavailable => '运行时不可用';

  @override
  String get wslcUnknownAction => '未知操作';

  @override
  String wslcCheckTheResultOfTheCommandWillNot(Object value, Object value2) {
    return '请检查$value的结果。命令不会自动重试。$value2';
  }

  @override
  String wslcCheckTheResult(Object label) {
    return '$label：请检查结果';
  }

  @override
  String get wslcCheckTheResultsOfSomeOperationsCommandsWill => '部分操作的结果需要检查。命令不会自动重试。';

  @override
  String get wslcActionCurrentlyUnavailable => '操作当前不可用';

  @override
  String get wslcStart => '启动';

  @override
  String get wslcStop => '停止';

  @override
  String get wslcRestart => '重启';

  @override
  String wslcCreated(Object value) {
    return '创建于 $value';
  }

  @override
  String wslcRemoveSelectedItems(Object length) {
    return '删除所选项目（$length）？';
  }

  @override
  String get wslcDataInTheSelectedVolumesWillBeLost => '所选卷中的数据将丢失。';

  @override
  String get wslcRemovalCannotBeUndone => '删除操作无法撤销。';

  @override
  String wslcRemoved(Object removed) {
    return '已删除：$removed';
  }

  @override
  String wslcRemovedOf(Object removed, Object length) {
    return '已删除 $removed/$length';
  }

  @override
  String get wslcLookingForItemsToRemove => '正在查找可删除的项目…';

  @override
  String get wslcCouldNotLoadTheList => '无法加载列表';

  @override
  String get wslcEverythingIsClean => '已清理完毕';

  @override
  String wslcUpToCanBeFreed(Object value) {
    return '最多可释放 $value';
  }

  @override
  String wslcFound(Object length) {
    return '已找到：$length';
  }

  @override
  String wslcOf(Object value, Object value2) {
    return '$value/$value2';
  }

  @override
  String get wslcCleanUpSelectedItems => '清理所选项目';

  @override
  String wslcCleanUpSelectedItemsCleanupPage(Object length, Object value2) {
    return '清理所选项目（$length$value2）';
  }

  @override
  String get wslcNothingToCleanUp => '没有可清理的项目';

  @override
  String get wslcThereAreNoStoppedContainersAllImagesVolumes => '没有已停止的容器。所有镜像、卷和网络都在使用中。';

  @override
  String get wslcLogStreamReading => '正在读取日志';

  @override
  String get wslcLogStreamTimedOut => '日志流读取超时';

  @override
  String get wslcStoppedContainers => '已停止的容器';

  @override
  String get wslcUnusedImages => '未使用的镜像';

  @override
  String get wslcUnusedVolumes => '未使用的卷';

  @override
  String get wslcEmptyNetworks => '空网络';

  @override
  String get wslcRemovesTheContainerFilesystemNamedVolumesAreKept => '将删除容器文件系统，保留命名卷。';

  @override
  String get wslcNoContainersUseTheseImagesYouCanDownload => '没有容器使用这些镜像，可重新下载。';

  @override
  String get wslcTheseVolumesAreNotAttachedToAnyContainer => '这些卷未挂载到容器。删除卷会清除其中的数据，请先备份需要保留的内容。';

  @override
  String get wslcUserDefinedNetworksWithNoAttachedContainers => '没有连接容器的自定义网络。';

  @override
  String get wslcDockerComposeFile => 'Docker Compose 文件';

  @override
  String get wslcComposeProjectDirectory => 'Compose 项目目录';

  @override
  String get wslcPasteDockerComposeYmlToStartTheServices => '粘贴 docker-compose.yml 即可启动服务，效果与 docker compose up -d 相同';

  @override
  String get wslcOpenFile => '打开文件…';

  @override
  String get wslcBackToFile => '返回文件';

  @override
  String get wslcStopping => '正在停止…';

  @override
  String get wslcRebuildImagesBuild => '重新构建镜像（--build）';

  @override
  String get wslcStopAndRemove => '停止并删除';

  @override
  String get wslcFromNameOrTheDirectory => '取自 name: 或目录名';

  @override
  String get wslcProjectDirectory => '项目目录';

  @override
  String get wslcRelativePathsEnvEnvFileAndBuild => '相对路径、.env、env_file 和 build';

  @override
  String get wslcChooseDirectory => '选择目录';

  @override
  String get wslcPasteDockerComposeYml => '粘贴 docker-compose.yml';

  @override
  String get wslcOrOpenAFileToPreviewThePlan => '或打开文件以在启动前预览执行计划';

  @override
  String get wslcParsingFile => '正在解析文件…';

  @override
  String wslcProject(Object project) {
    return '项目 $project';
  }

  @override
  String get wslcService => '个服务';

  @override
  String get wslcServices => '个服务';

  @override
  String get wslcServicesComposeDialog => '个服务';

  @override
  String get wslcDifferencesFromDocker => '与 Docker 的差异';

  @override
  String get wslcServicesInStartupOrder => '服务（按启动顺序）';

  @override
  String get wslcNetworksAndVolumes => '网络和卷';

  @override
  String get wslcExternal => ' · 外部';

  @override
  String get wslcExternalComposeDialog => ' · 外部';

  @override
  String wslcWSLcCommands(Object length) {
    return 'WSLc 命令（$length）';
  }

  @override
  String wslcBuild(Object image) {
    return '构建 → $image';
  }

  @override
  String wslcVariables(Object length) {
    return '变量数：$length';
  }

  @override
  String wslcAfter(Object service, Object value2) {
    return '在 $service$value2 之后';
  }

  @override
  String get wslcHealthy => '（健康）';

  @override
  String get wslcCompleted => '（已完成）';

  @override
  String wslcProjectReady(Object project) {
    return '项目已就绪：$project';
  }

  @override
  String wslcStopAndRemoveComposeDialog(Object project) {
    return '停止并删除 $project？';
  }

  @override
  String get wslcAsWithDockerComposeDownThisStopsAnd => '与 docker compose down 一样，将停止并删除项目容器及其创建的网络，保留镜像。';

  @override
  String wslcAlsoRemoveProjectVolumes(Object ownVolumes) {
    return '同时删除项目卷（$ownVolumes）';
  }

  @override
  String get wslcDataInTheseVolumesWillBeLostV => '这些卷中的数据将丢失（-v）';

  @override
  String get wslcCancelled => '已取消';

  @override
  String get wslcStoppedOnRequest => '已按请求停止';

  @override
  String get wslcDirectoriesForMounts => '挂载目录';

  @override
  String wslcNetwork(Object name) {
    return '网络 $name';
  }

  @override
  String get wslcExternalNetworkFound => '已找到外部网络';

  @override
  String get wslcAlreadyExists => '已存在';

  @override
  String wslcExternalNetworkNotFound(Object name) {
    return '找不到外部网络 $name';
  }

  @override
  String get wslcNetworkCreated => '已创建网络';

  @override
  String wslcVolume(Object name) {
    return '卷 $name';
  }

  @override
  String get wslcExternalVolumeFound => '已找到外部卷';

  @override
  String wslcExternalVolumeNotFound(Object name) {
    return '找不到外部卷 $name';
  }

  @override
  String get wslcVolumeCreated => '已创建卷';

  @override
  String wslcBuilding(Object image) {
    return '正在构建 $image';
  }

  @override
  String get wslcImageAlreadyBuilt => '镜像已构建';

  @override
  String get wslcBuilt => '构建完成';

  @override
  String wslcImage(Object image) {
    return '镜像 $image';
  }

  @override
  String get wslcAlreadyDownloaded => '已下载';

  @override
  String get wslcDownloaded => '下载完成';

  @override
  String get wslcWindowsFolder => 'Windows 文件夹';

  @override
  String get wslcUnknownType => '未知类型';

  @override
  String get wslcShowInFileExplorer => '在文件资源管理器中显示';

  @override
  String get wslcThisVolumeIsStoredInsideTheWSLcVM => '该卷存储在 WSLc 虚拟机内。打开容器文件即可查看其内容。';

  @override
  String get wslcShowFiles => '显示文件';

  @override
  String get wslcCopyWindowsPath => '复制 Windows 路径';

  @override
  String get wslcCopyVolumeName => '复制卷名';

  @override
  String get wslcCopyPathInsideContainer => '复制容器内路径';

  @override
  String get wslcShowFilesInContainer => '查看容器中的文件';

  @override
  String get wslcOpenTerminalHere => '在此处打开终端';

  @override
  String get wslcReadingEnvironment => '正在读取环境变量…';

  @override
  String get wslcCouldNotReadTheEnvironment => '无法读取环境变量';

  @override
  String get wslcSearchByVariableName => '按变量名搜索';

  @override
  String wslcHideVariablesFromTheImage(Object inherited) {
    return '隐藏镜像自带的变量（$inherited）';
  }

  @override
  String get wslcHideValues => '隐藏值';

  @override
  String get wslcShowAll => '全部显示';

  @override
  String get wslcCopyAsEnv => '复制为 .env';

  @override
  String get wslcNoEnvironmentVariables => '没有环境变量';

  @override
  String get wslcEmpty => '（空）';

  @override
  String get wslcFromImage => '来自镜像';

  @override
  String get wslcCopyValue => '复制值';

  @override
  String wslcValueOf(Object key) {
    return '$key 的值';
  }

  @override
  String get wslcValuesAreReadWhenYouOpenThisTab => '打开此标签页时才会读取值，值不会保存。锁形图标表示变量可能包含密钥等敏感信息。';

  @override
  String get wslcUploadFileToContainer => '上传文件到容器';

  @override
  String get wslcReplaceFile => '替换文件？';

  @override
  String wslcAlreadyExistsAndWillBeOverwritten(Object target) {
    return '$target 已存在，将被覆盖。';
  }

  @override
  String wslcUploaded(Object target) {
    return '已上传：$target';
  }

  @override
  String get wslcFilesAreAvailableWhileTheContainerIsRunning => '容器运行时可查看文件';

  @override
  String get wslcStartTheContainerToBrowseItsFilesystem => '启动容器以浏览其文件系统';

  @override
  String get wslcCopyPath => '复制路径';

  @override
  String get wslcUploadAFileFromThisComputer => '从本机上传文件到此处';

  @override
  String get wslcOpenTerminalInThisDirectory => '在此目录打开终端';

  @override
  String get wslcReadingDirectory => '正在读取目录…';

  @override
  String get wslcCouldNotReadDirectory => '无法读取目录';

  @override
  String get wslcLink => '链接';

  @override
  String get wslcDownloadToComputer => '下载到本机';

  @override
  String get wslcCloseWithoutSaving => '关闭且不保存？';

  @override
  String get wslcChangesToThisFileWillBeLost => '对此文件所做的更改将丢失。';

  @override
  String get wslcGoBack => '返回';

  @override
  String get wslcEdit => '编辑';

  @override
  String get wslcDiscardEdits => '放弃更改';

  @override
  String get wslcReadingFile => '正在读取文件…';

  @override
  String get wslcCannotOpenThisFileAsText => '无法以文本方式打开此文件';

  @override
  String get wslcUTFTextFilesUpToMB => '最大 2 MB 的 UTF-8 文本文件';

  @override
  String get wslcLine => '行';

  @override
  String get wslcLines => '行';

  @override
  String get wslcLinesFileEditor => '行';

  @override
  String get wslcEditing => ' · 编辑模式';

  @override
  String get wslcReadOnly => ' · 只读';

  @override
  String get wslcUnsavedChanges => ' · 有未保存的更改';

  @override
  String get wslcPowerShellCommandSettingsThatMatchTheImageCMD => 'PowerShell 命令。与镜像一致的配置（CMD、PATH 和工作目录）已省略。';

  @override
  String wslcIncludeEnvironmentVariableValues(Object envCount) {
    return '包含环境变量值（$envCount）';
  }

  @override
  String get wslcValuesMayContainPasswordsAndTokens => '这些值可能包含密码和令牌';

  @override
  String get wslcCloneInWizard => '在向导中克隆';

  @override
  String get wslcCopy => '复制';

  @override
  String wslcCommand(Object message) {
    return '命令：$message';
  }

  @override
  String get wslcDirectoryToMount => '要挂载的目录';

  @override
  String get wslcFromCompose => '从 Compose 导入…';

  @override
  String get wslcNameOptional => '名称（可选）';

  @override
  String get wslcTheImageIsNotAvailableLocallyWSLcWill => '本地没有此镜像。WSLc 将在创建容器时拉取镜像。';

  @override
  String get wslcThisComputerOnlyTheWholeNetwork => '127.0.0.1：仅本机。0.0.0.0：整个网络。';

  @override
  String get wslcHostPort => '主机端口';

  @override
  String get wslcContainerPort => '容器端口';

  @override
  String get wslcDirectoriesAndVolumes => '目录和卷';

  @override
  String get wslcWindowsDirectoryOrNamedVolumePathInContainer => 'Windows 目录或命名卷 → 容器内路径';

  @override
  String get wslcVolumeRunWizard => '卷';

  @override
  String get wslcVolumeNameCreatedIfNeeded => '卷名（需要时会创建）';

  @override
  String get wslcCPathToFolder => 'C:\\path\\to\\folder';

  @override
  String get wslcPathInContainer => '/path/in/container';

  @override
  String get wslcPassedThroughATemporaryFileRatherThanThe => '通过临时文件传递，不写入命令行';

  @override
  String get wslcVariable => '变量';

  @override
  String get wslcNAME => '名称';

  @override
  String get wslcAdvanced => '高级';

  @override
  String get wslcCommandOverridesTheImageCMD => '命令（覆盖镜像的 CMD）';

  @override
  String get wslcWorkingDirectory => '工作目录';

  @override
  String get wslcRemoveAfterStoppingRm => '停止后删除（--rm）';

  @override
  String get wslcCreateWithoutStarting => '仅创建，不启动';

  @override
  String get wslcCreatingPullingTheImageIfItIsMissing => '正在创建… 若本地缺少镜像，则会自动拉取。';

  @override
  String get wslcCreateAndStart => '创建并启动';

  @override
  String get wslcRemove => '移除';

  @override
  String wslcTemplateSaved(Object name) {
    return '已保存模板“$name”';
  }

  @override
  String get wslcSaveAsTemplate => '另存为模板';

  @override
  String get wslcATemplateWithThisNameWillBeReplaced => '同名模板将被替换';

  @override
  String get wslcSaveEnvironmentVariableValues => '保存环境变量值';

  @override
  String get wslcOtherwiseOnlyNamesAreSavedEnterTheValues => '未选择时仅保存变量名，创建容器时再填写值。';

  @override
  String get wslcTheTemplateExcludesTheContainerNameTemplatesAre => '模板不包含容器名称。模板在本设备的当前配置文件中加密保存，不会发送到服务器。';

  @override
  String get wslcTemplates => '模板';

  @override
  String get wslcSaveAsTemplateTemplates => '另存为模板…';

  @override
  String get wslcTheTerminalIsAvailableWhileTheContainerIs => '容器运行时可打开终端';

  @override
  String get wslcSessionEnded => '会话已结束';

  @override
  String wslcWslcExecItBashOrSh(Object cols, Object rows) {
    return 'wslc exec -it · bash 或 sh · $cols×$rows';
  }

  @override
  String get wslcReconnect => '重新连接';

  @override
  String get wslcNewSession => '新建会话';

  @override
  String get wslcOpenInSeparateWindow => '在单独窗口中打开';

  @override
  String get wslcCopyTheSelectionWithCtrlShiftCAnd => '按 Ctrl+Shift+C 复制选区，按 Ctrl+Shift+V 粘贴。关闭标签页会结束本次会话，容器将继续运行。';

  @override
  String wslcPulling(Object image) {
    return '正在拉取 $image';
  }

  @override
  String get wslcImageDownloaded => '镜像已下载';

  @override
  String get wslcPullImage => '拉取镜像';

  @override
  String get wslcDownload => '下载';

  @override
  String get wslcImageImageDialogs => '镜像';

  @override
  String get wslcImageBuilt => '镜像已构建';

  @override
  String get wslcBuildContextDirectory => '构建上下文目录';

  @override
  String wslcBuildArgumentMustUseKEYVALUE(Object t) {
    return '构建参数“$t”必须采用 KEY=VALUE 格式';
  }

  @override
  String get wslcChooseTheBuildContextDirectory => '选择构建上下文目录';

  @override
  String get wslcBuildImage => '构建镜像';

  @override
  String get wslcDockerfileOptionalDefaultsToTheOneInThe => 'Dockerfile（可选，默认使用构建上下文中的文件）';

  @override
  String get wslcCommaSeparatedTags => '以逗号分隔的标签';

  @override
  String get wslcBuildArgumentsOneKEYVALUEPerLine => '构建参数，每行一个 KEY=VALUE';

  @override
  String get wslcTargetStageOptional => '目标阶段（可选）';

  @override
  String get wslcDisableCache => '禁用缓存';

  @override
  String get wslcPullUpdatedBaseImages => '拉取最新基础镜像';

  @override
  String get wslcBuildImageDialogs => '构建';

  @override
  String get wslcCreateContainer => '创建容器…';

  @override
  String get wslcAddTag => '添加标签…';

  @override
  String get wslcPushToRegistry => '推送到仓库…';

  @override
  String get wslcSaveAsTar => '另存为 tar…';

  @override
  String wslcNewTagFor(Object source) {
    return '$source 的新标签';
  }

  @override
  String wslcAddedTag(Object target) {
    return '已添加标签 $target';
  }

  @override
  String get wslcPushImage => '推送镜像？';

  @override
  String wslcTheImageWillBePushedToTheRegistry(Object image) {
    return '$image\n\n镜像将推送到其名称指定的镜像仓库。如果需要身份验证，请先在“镜像仓库”中登录。';
  }

  @override
  String wslcPushing(Object image) {
    return '正在推送 $image';
  }

  @override
  String get wslcImagePushed => '镜像已推送';

  @override
  String get wslcSignedIn => '已登录';

  @override
  String wslcSignedOutOf(Object name) {
    return '已退出 $name';
  }

  @override
  String get wslcRegistries => '镜像仓库';

  @override
  String get wslcServerEGGhcrIoLeaveBlankFor => '服务器，例如 ghcr.io（留空使用默认值）';

  @override
  String get wslcPasswordOrAccessToken => '密码或访问令牌';

  @override
  String get wslcThePasswordIsPassedToWSLcThroughStdin => '密码通过标准输入传递给 WSLc，应用不会保存密码。';

  @override
  String get wslcSignedInDuringThisSession => '本次会话中已登录';

  @override
  String get wslcSaveImageAsTar => '将镜像保存为 tar';

  @override
  String wslcSaving(Object ref) {
    return '正在保存 $ref';
  }

  @override
  String get wslcImageTarArchiveWslcSaveDockerSave => '镜像 tar 归档（wslc save / docker save）';

  @override
  String wslcLoadingImagesFrom(Object value) {
    return '正在从 $value 加载镜像';
  }

  @override
  String get wslcImagesLoaded => '镜像已加载';

  @override
  String get wslcFilesystemTarArchiveWslcExport => '文件系统 tar 归档（wslc export）';

  @override
  String get wslcNewImageName => '新镜像名称';

  @override
  String get wslcMyImportedOptional => 'my/imported:1（可选）';

  @override
  String wslcImporting(Object value) {
    return '正在导入 $value';
  }

  @override
  String get wslcImageCreated => '镜像已创建';

  @override
  String get wslcImportFromArchives => '从归档导入';

  @override
  String get wslcLoadImagesFromTarLoad => '从 tar 加载镜像（load）…';

  @override
  String get wslcImportFilesystemFromTarImport => '从 tar 导入文件系统（import）…';

  @override
  String get wslcSearchByRepositoryTagOrID => '按仓库、标签或 ID 搜索';

  @override
  String get wslcNoImagesYet => '暂无镜像';

  @override
  String get wslcUntagged => '无标签';

  @override
  String wslcContainers(Object containers) {
    return '容器数：$containers';
  }

  @override
  String get wslcSizeUnknown => '大小未知';

  @override
  String wslcImageInventoryPages(Object value, Object value2) {
    return '镜像 $value$value2';
  }

  @override
  String get wslcDeleteImage => '删除镜像';

  @override
  String get wslcDeleteImageInventoryPages => '删除镜像？';

  @override
  String get wslcRemoveImageTag => '移除镜像标签？';

  @override
  String get wslcUnnamed => '未命名';

  @override
  String wslcTheImageIsUsedByContainerSThe(Object containers) {
    return '有 $containers 个容器使用此镜像。只要这些容器还存在，运行时就会拒绝删除镜像。 ';
  }

  @override
  String get wslcThisCannotBeUndoneYouWillNeedTo => '此操作无法撤销。之后如需使用，必须重新拉取镜像。';

  @override
  String wslcOnlyTheTagWillBeRemovedTheLayers(Object value) {
    return '仅移除标签 $value。镜像的所有标签都被移除后，其镜像层才会被删除。';
  }

  @override
  String get wslcImageDeleted => '镜像已删除';

  @override
  String get wslcSearchByVolumeName => '按卷名称搜索';

  @override
  String get wslcNoVolumesYet => '暂无卷';

  @override
  String get wslcCreateVolume => '创建卷';

  @override
  String wslcDriver(Object value) {
    return '驱动程序 $value';
  }

  @override
  String get wslcSizeUnavailable => '大小不可用';

  @override
  String get wslcDeleteVolume => '删除卷';

  @override
  String get wslcDeleteVolumeInventoryPages => '删除卷？';

  @override
  String wslcAllDataInThisVolumeWillBePermanently(Object name) {
    return '$name\n\n此卷上的所有数据将被永久删除。如果卷已挂载到容器，运行时会拒绝删除。';
  }

  @override
  String wslcVolumeDeleted(Object name) {
    return '卷 $name 已删除';
  }

  @override
  String get wslcYes => '是';

  @override
  String get wslcNo => '否';

  @override
  String get wslcSearchByNameIDOrDriver => '按名称、ID 或驱动程序搜索';

  @override
  String get wslcNoNetworksFound => '未找到网络';

  @override
  String get wslcCreateNetwork => '创建网络';

  @override
  String get wslcInternal => '内部';

  @override
  String get wslcConnectContainer => '连接容器';

  @override
  String get wslcDeleteNetwork => '删除网络';

  @override
  String get wslcDeleteNetworkInventoryPages => '删除网络？';

  @override
  String wslcTheRuntimeWillRefuseToDeleteThisNetwork(Object name) {
    return '$name\n\n如果仍有容器连接到此网络，运行时会拒绝删除。';
  }

  @override
  String wslcNetworkDeleted(Object name) {
    return '网络 $name 已删除';
  }

  @override
  String wslcVolumeCreatedResourceDialogs(Object name) {
    return '卷 $name 已创建';
  }

  @override
  String get wslcNewVolume => '新建卷';

  @override
  String get wslcEnterAVolumeName => '请输入卷名称';

  @override
  String get wslcOptionsOneKEYVALUEPerLine => '每行输入一个 KEY=VALUE 选项';

  @override
  String get wslcVolumeNameEGAppData => '卷名称，例如 app-data';

  @override
  String get wslcGuestInTheVMFilesystemDefault => 'guest — 存储在虚拟机文件系统中（默认）';

  @override
  String get wslcVhdSeparateVirtualDisk => 'vhd — 使用独立虚拟磁盘';

  @override
  String get wslcDriverOptionsKEYVALUEOptional => '驱动程序选项 KEY=VALUE（可选）';

  @override
  String wslcNetworkCreatedResourceDialogs(Object name) {
    return '网络 $name 已创建';
  }

  @override
  String get wslcNewNetwork => '新建网络';

  @override
  String get wslcEnterANetworkName => '请输入网络名称';

  @override
  String get wslcNetworkNameEGBackend => '网络名称，例如 backend';

  @override
  String get wslcSubnetCIDREGOptional => '子网 CIDR，例如 10.231.0.0/24（可选）';

  @override
  String get wslcGatewayEG => '网关，例如 10.231.0.1';

  @override
  String get wslcContainerAddressRangeInCIDRNotationOptional => '容器地址范围，使用 CIDR 表示（可选）';

  @override
  String get wslcInternalNetworkWithoutExternalAccess => '内部网络，无法访问外部网络';

  @override
  String wslcContainerConnectedToNetwork(Object net) {
    return '容器已连接到网络 $net';
  }

  @override
  String get wslcConnect => '连接';

  @override
  String get wslcSelectANetworkAndAContainer => '请选择网络和容器';

  @override
  String get wslcNetworkAliasOptional => '网络别名（可选）';

  @override
  String get wslcStaticIPvAddressFromTheSubnetOptional => '子网内的静态 IPv4 地址（可选）';

  @override
  String wslcContainerDisconnectedFromNetwork(Object network) {
    return '容器已与网络 $network 断开连接';
  }

  @override
  String wslcBackingUpVolume(Object volume) {
    return '正在备份卷 $volume';
  }

  @override
  String wslcBackupSavedTo(Object path) {
    return '备份已保存到 $path';
  }

  @override
  String get wslcVolumeArchiveTarGz => '卷归档文件（.tar.gz）';

  @override
  String wslcRestoringVolume(Object target) {
    return '正在还原卷 $target';
  }

  @override
  String wslcVolumeRestored(Object target) {
    return '卷 $target 已还原';
  }

  @override
  String get wslcRestoreToANewVolume => '还原到新卷';

  @override
  String get wslcRestoreVolume => '还原卷';

  @override
  String get wslcVolumeNameCreatedIfItDoesNotExist => '卷名称（如果不存在则创建）';

  @override
  String get wslcClearVolumeBeforeRestoring => '还原前清空卷';

  @override
  String get wslcOtherwiseArchiveFilesAreMergedIntoTheExisting => '否则，归档中的文件将写入现有卷并覆盖同名文件';

  @override
  String get wslcBackUpToArchive => '备份到归档文件…';

  @override
  String get wslcRestoreFromArchive => '从归档文件还原…';

  @override
  String get wslcSearchLoadedLogs => '搜索已加载的日志';

  @override
  String get wslcPauseAutoscroll => '暂停自动滚动';

  @override
  String get wslcEnableAutoscroll => '启用自动滚动';

  @override
  String get wslcClearView => '清空当前视图';

  @override
  String get wslcCopyLoadedLogs => '复制已加载的日志';

  @override
  String get wslcSaveLoadedLogs => '保存已加载的日志';

  @override
  String get wslcLinesLogPanel => '1000 行';

  @override
  String get wslcMin => '5 分钟';

  @override
  String get wslcHour => '1 小时';

  @override
  String get wslcHours => '24 小时';

  @override
  String get wslcAll => '全部';

  @override
  String get wslcAllStreams => '所有流';

  @override
  String get wslcShowRuntimeTimestampsInLocalTime => '以本地时间显示运行时的时间戳';

  @override
  String get wslcFormatLinesContainingJSONObjects => '格式化包含 JSON 对象的日志行';

  @override
  String get wslcNoMatches => '无匹配项';

  @override
  String get wslcTruncated => ' … [已截断]';

  @override
  String wslcShown(Object length) {
    return '已显示：$length';
  }

  @override
  String wslcEvicted(Object value) {
    return '已移出缓冲区：$value';
  }

  @override
  String wslcTruncatedLogPanel(Object truncated) {
    return '已截断：$truncated';
  }

  @override
  String get wslcLogsMayContainSecrets => '日志可能包含机密信息';

  @override
  String get wslcEnterHexDigitsEGBFF => '请输入 6 位十六进制数，例如 #4B89FF';

  @override
  String wslcColorSpectrum(Object value) {
    return '色谱 — $value';
  }

  @override
  String wslcHueDegrees(Object value) {
    return '色相：$value 度';
  }

  @override
  String wslcSaturationPercent(Object value) {
    return '饱和度：$value%';
  }

  @override
  String wslcBrightnessPercent(Object value) {
    return '亮度：$value%';
  }

  @override
  String get wslcSelect => '选择';

  @override
  String get wslcWSLcConnection => 'WSLc 连接';

  @override
  String get wslcEnterThePathToTheInstalledWslcExe => '输入已安装的 wslc.exe 的路径，或留空以使用标准 WSL 目录。';

  @override
  String get wslcPathToWslcExe => 'wslc.exe 路径';

  @override
  String get wslcAvailable => '可用';

  @override
  String get wslcUnavailable => '不可用';

  @override
  String get wslcRefreshData => '刷新数据';

  @override
  String wslcEveryS(Object seconds) {
    return '每 $seconds 秒';
  }

  @override
  String wslcSavedRunTemplates(Object length) {
    return '已保存的运行模板：$length';
  }

  @override
  String get wslcTemplatesAreStoredInThisProfileSEncrypted =>
      '模板存储在此设备上当前配置文件的加密数据库中。外观和剪贴板清除选项可在 ConsoleCrypt 通用设置中调整。';

  @override
  String get wslcBuildingMap => '正在构建拓扑图…';

  @override
  String get wslcCouldNotBuildMap => '无法构建拓扑图';

  @override
  String get wslcWindowsFolderTopologyPage => 'Windows 文件夹';

  @override
  String get wslcOpenCleanup => '打开清理工具';

  @override
  String wslcUnused(Object length) {
    return '未使用：$length';
  }

  @override
  String get wslcNoConnectionsYet => '暂无连接';

  @override
  String get wslcVolumesFoldersAndNetworks => '卷、文件夹和网络';

  @override
  String get wslcRunning => '正在运行…';

  @override
  String wslcErrorExitCode(Object exitCode) {
    return '错误，退出代码 $exitCode';
  }

  @override
  String get wslcWaitingForOutput => '正在等待输出…';

  @override
  String wslcPortIsNotPublishedOnThisComputer(Object containerPort) {
    return '$containerPort：端口未发布到本机';
  }

  @override
  String get wslcOpenOrCopyPortAddress => '打开或复制端口地址';

  @override
  String wslcCopyPorts(Object value) {
    return '复制 $value';
  }

  @override
  String wslcCopyLocalhost(Object hostPort) {
    return '复制 localhost:$hostPort';
  }

  @override
  String wslcCopyPortsOther(Object address, Object hostPort) {
    return '复制 $address:$hostPort';
  }

  @override
  String wslcThisPortIsPublishedOnlyOnAndCannot(Object hostIp, Object hostPort, Object value3) {
    return '此端口仅发布在 $hostIp，无法通过本机的 IP 地址访问。如需从网络访问，请将其发布为 0.0.0.0:$hostPort:$value3。';
  }

  @override
  String get wslcUnclosedQuote => '引号未闭合';

  @override
  String get wslcKB => 'KB';

  @override
  String get wslcMB => 'MB';

  @override
  String get wslcGB => 'GB';

  @override
  String get wslcTB => 'TB';

  @override
  String get wslcS => '/秒';

  @override
  String get wslcCollectingData => '正在收集数据…';

  @override
  String get wslcCopyFullID => '复制完整 ID';

  @override
  String get wslcWSLcSessionClosed => 'WSLc 会话已关闭';

  @override
  String get wslcShowContainerData => '显示容器数据';

  @override
  String get wslcConfirmAccessToTheHiddenWSLcEnvironmentVariables => '请确认是否访问 WSLc 隐藏的环境变量。';

  @override
  String get wslcContainerEngines => '容器引擎';

  @override
  String get wslcConnections => '连接';

  @override
  String get wslcCleanup => '清理';

  @override
  String get wslcOrphanedProjectContainers => '项目孤立容器';

  @override
  String wslcTheirServicesAreAbsentFromTheFileSo(Object value) {
    return '$value：文件中没有对应服务，已保留容器';
  }

  @override
  String get wslcUnchangedAlreadyRunning => '配置未变，已在运行';

  @override
  String get wslcStarting => '正在启动…';

  @override
  String get wslcUnchangedStarted => '配置未变，已启动';

  @override
  String get wslcConfigurationChangedRecreating => '配置已更改，正在重新创建…';

  @override
  String wslcTheNameIsInUseByAContainer(Object containerName) {
    return '名称 $containerName 已被其他项目的容器使用';
  }

  @override
  String get wslcCreating => '正在创建…';

  @override
  String wslcConnectingTo(Object network) {
    return '正在连接到 $network…';
  }

  @override
  String get wslcCreatedAndStarted => '已创建并启动';

  @override
  String get wslcRecreatedAndStarted => '已重新创建并启动';

  @override
  String wslcWaitingForToBecomeHealthy(Object service) {
    return '正在等待 $service 通过健康检查…';
  }

  @override
  String wslcWaitingForToFinish(Object service) {
    return '正在等待 $service 结束…';
  }

  @override
  String wslcIsUnhealthyDependentServicesDidNotStart(Object service) {
    return '$service 未通过健康检查。依赖该服务的服务未启动。';
  }

  @override
  String wslcStoppedBeforeBecomingHealthy(Object service) {
    return '$service 在通过健康检查前已停止';
  }

  @override
  String wslcHasNoHealthCheckAndCannotSatisfyService(Object service) {
    return '$service 没有健康检查，无法满足 service_healthy';
  }

  @override
  String wslcExitedWithCode(Object service, Object exitCode) {
    return '$service 已退出，退出码为 $exitCode';
  }

  @override
  String wslcTimedOutAfterMin(Object service, Object inMinutes) {
    return '$service：等待 $inMinutes 分钟后超时';
  }

  @override
  String wslcExitedWithCodeComposeRunner(Object value, Object value2) {
    return '已退出，退出码为 $value$value2';
  }

  @override
  String get wslcProjectContainers => '项目容器';

  @override
  String get wslcNoneFound => '未找到';

  @override
  String wslcRemoving(Object name) {
    return '正在删除 $name';
  }

  @override
  String get wslcStoppedAndRemoved => '已停止并删除';

  @override
  String get wslcAlreadyGone => '已不存在';

  @override
  String get wslcRemovedComposeRunner => '已删除';

  @override
  String get wslcRemovedComposeRunnerOther => '已删除';

  @override
  String wslcStopTheSelectedContainers(Object length) {
    return '停止所选容器（$length）？';
  }

  @override
  String wslcRestartTheSelectedContainers(Object length) {
    return '重启所选容器（$length）？';
  }

  @override
  String wslcRemoveTheSelectedContainers(Object length) {
    return '删除所选容器（$length）？';
  }

  @override
  String wslcRemovalCannotBeUndoneNamedVolumesWillBe(Object value) {
    return '$value删除操作无法撤销。命名卷将保留。';
  }

  @override
  String get wslcRunningContainersWillBeForciblyStopped => '运行中的容器将被强制停止。';

  @override
  String get wslcTheRuntimeMayRemoveContainersWithAutoRemove => '运行时可能在容器停止后删除已启用自动删除的容器。';

  @override
  String wslcDoneOf(Object done, Object length, Object value3) {
    return '已完成：$done/$length$value3';
  }

  @override
  String get wslcRunningContainersPage => '运行中';

  @override
  String get wslcSearchByNameIDImageOrPort => '按名称、ID、镜像或端口搜索';

  @override
  String get wslcRunningContainersPageOther => '运行中';

  @override
  String get wslcStopped => '已停止';

  @override
  String get wslcNewContainer => '新建容器';

  @override
  String get wslcDockerComposeStartFromYAML => 'Docker Compose：从 YAML 启动';

  @override
  String get wslcBulkActions => '批量操作';

  @override
  String wslcStopAllRunningContainers(Object running) {
    return '停止所有运行中的容器（$running）';
  }

  @override
  String wslcStartAllStoppedContainers(Object stopped) {
    return '启动所有已停止的容器（$stopped）';
  }

  @override
  String get wslcNotConnectedToWSLc => '未连接到 WSLc';

  @override
  String get wslcNoContainersYet => '暂无容器';

  @override
  String get wslcCheckTheConnectionInSettings => '请在设置中检查连接';

  @override
  String get wslcSelectNewToCreateYourFirstContainer => '选择“新建”以创建第一个容器';

  @override
  String get wslcChangeTheSearchTermOrFilter => '请更改搜索词或筛选条件';

  @override
  String wslcSelected(Object count) {
    return '已选择：$count';
  }

  @override
  String wslcUnknown(Object value) {
    return '未知（$value）';
  }

  @override
  String get wslcStopThisContainer => '停止此容器？';

  @override
  String get wslcRestartThisContainer => '重启此容器？';

  @override
  String get wslcAutoRemoveIsEnabledTheRuntimeMayRemove => '已启用自动删除。运行时可能在容器停止后将其删除。';

  @override
  String get wslcAutoRemoveIsDisabled => '自动删除已禁用。';

  @override
  String get wslcTheAutoRemoveSettingIsUnknownTheRuntime => '自动删除设置未知。运行时可能在容器停止后将其删除。';

  @override
  String get wslcRemoveThisContainer => '删除此容器？';

  @override
  String wslcRemovalCannotBeUndoneNamedVolumesWillBeContainerActions(Object name, Object id, Object value3) {
    return '$name\n$id\n\n$value3删除操作无法撤销。命名卷将保留。';
  }

  @override
  String get wslcTheContainerIsRunningItWillBeForcibly => '容器正在运行。它将被强制停止并删除。';

  @override
  String wslcContainerRemoved(Object name) {
    return '已删除容器 $name';
  }

  @override
  String get wslcMoreActions => '更多操作';

  @override
  String get wslcOpenInBrowser => '在浏览器中打开';

  @override
  String wslcOpen(Object value) {
    return '打开 $value';
  }

  @override
  String get wslcDetails => '详情';

  @override
  String get wslcStartupCommand => '启动命令…';

  @override
  String get wslcExportFilesystemAsTar => '将文件系统导出为 tar…';

  @override
  String get wslcCopyID => '复制 ID';

  @override
  String get wslcCopyName => '复制名称';

  @override
  String get wslcExportContainerFilesystem => '导出容器文件系统';

  @override
  String wslcExporting(Object name) {
    return '正在导出 $name';
  }

  @override
  String wslcSaved(Object path) {
    return '已保存：$path';
  }

  @override
  String get wslcBackToList => '返回列表';

  @override
  String get wslcStartupCommandContainerDetailsPage => '启动命令';

  @override
  String get wslcRemoveContainer => '删除容器';

  @override
  String get wslcContainerNoLongerFound => '找不到该容器';

  @override
  String get wslcTheRuntimeMayHaveRemovedItWhenIt => '运行时可能已在容器停止后将其删除';

  @override
  String get wslcLoadingInspectData => '正在加载检查数据…';

  @override
  String get wslcCouldNotLoadInspectData => '无法加载检查数据';

  @override
  String get wslcExitCode => '退出码';

  @override
  String get wslcFinished => '已结束';

  @override
  String get wslcUnknownContainerDetailsPage => '未知';

  @override
  String get wslcEnabled => '已启用';

  @override
  String get wslcDisabled => '已禁用';

  @override
  String get wslcStopTimeout => '停止超时';

  @override
  String wslcSContainerDetailsPage(Object stopTimeoutSeconds) {
    return '$stopTimeoutSeconds 秒';
  }

  @override
  String get wslcReceiveInBlueSendInYellow => '蓝色表示接收，黄色表示发送';

  @override
  String get wslcDiskReadWrite => '磁盘读写';

  @override
  String get wslcProcesses => '进程';

  @override
  String get wslcNoPublishedPorts => '没有已发布的端口';

  @override
  String get wslcSelectAPortToOpenItInA => '选择端口可在浏览器中打开或复制其地址。';

  @override
  String get wslcStartup => '启动';

  @override
  String get wslcImageID => '镜像 ID';

  @override
  String get wslcConnectToNetwork => '连接网络';

  @override
  String get wslcTheRuntimeReturnedNoNetworkInformation => '运行时未返回网络信息';

  @override
  String get wslcDisconnectFromNetwork => '断开网络连接';

  @override
  String wslcGateway(Object gateway) {
    return '网关 $gateway';
  }

  @override
  String get wslcTheRuntimeReturnedNoMountInformation => '运行时未返回挂载信息';

  @override
  String get wslcNoMountedVolumesOrDirectories => '没有已挂载的卷或目录';

  @override
  String get navAreaAccess => '访问';

  @override
  String get navAreaWorkspaces => '工作区';

  @override
  String get navAreaProfile => '配置文件';

  @override
  String get hostInspector => '主机详情';

  @override
  String get captureTitle => 'TCPdump';

  @override
  String get captureSubtitle => '实时查看数据包头、地址和端口。';

  @override
  String get captureChoose => '选择端点以开始抓包。';

  @override
  String get captureListening => '正在监听数据包';

  @override
  String get captureStopped => '抓包已停止';

  @override
  String get captureMissing => '抓包端未安装 tcpdump。';

  @override
  String get captureDenied => '需要更高权限才能抓包。';

  @override
  String get captureStartTimeout => '服务器未在 20 秒内确认抓包已启动。';

  @override
  String get captureFailed => '未能完成抓包。';

  @override
  String get captureNoPeer => '无法确定连接地址。';

  @override
  String get captureSource => '抓包服务器';

  @override
  String get captureTarget => '对端服务器';

  @override
  String get captureServersHelp => '在第一台服务器上抓取它与第二台服务器之间的双向流量。';

  @override
  String get captureStart => '开始抓包';

  @override
  String get captureStop => '停止';

  @override
  String captureCount(int shown, int total) {
    return '显示 $shown / $total 个数据包';
  }

  @override
  String get captureInstall => '安装 tcpdump';

  @override
  String get captureManualInstall => '请使用此主机的包管理器安装 tcpdump，然后重新开始抓包。';

  @override
  String get captureSudo => '使用 sudo 重试';

  @override
  String get tmuxTitle => 'tmux 会话';

  @override
  String tmuxCount(int count) {
    return '$count 个会话';
  }

  @override
  String tmuxCounts(int windows, int clients) {
    return '窗口：$windows · 客户端：$clients';
  }

  @override
  String tmuxActivity(String time) {
    return '上次活动：$time';
  }

  @override
  String get tmuxAttach => '连接会话';

  @override
  String get tmuxDetach => '分离会话';

  @override
  String get tmuxCreate => '新建会话';

  @override
  String get tmuxKill => '终止会话';

  @override
  String tmuxConfirmKill(String name) {
    return '终止“$name”及其中的进程？';
  }

  @override
  String get tmuxEmpty => '此主机暂无 tmux 会话';

  @override
  String get tmuxError => 'tmux 操作失败。请刷新列表后重试。';

  @override
  String get tmuxNameHint => '请使用字母、数字、空格、连字符或下划线。';

  @override
  String get captureCheck => '检查 tcpdump';

  @override
  String get captureReady => '已安装 tcpdump';

  @override
  String get captureUseSudo => '使用 sudo 捕获';

  @override
  String get capturePrivilegeHint => '抓包需要 root 或抓包权限。若 sudo 提示，请输入密码。';

  @override
  String captureInstallHost(String host) {
    return '在 $host 上安装 tcpdump？命令将在其 SSH 终端中运行，可在终端中输入 sudo 密码并确认安装。';
  }

  @override
  String get captureExchange => '连接详情';

  @override
  String captureExchangeCount(int count) {
    return '选中时这两个端点之间的包数：$count';
  }

  @override
  String get registryBuildFromSource => '从本地 Dockerfile 构建';

  @override
  String get registryBuildFolder => '选择项目文件夹';

  @override
  String get registryBuildPush => '构建并发布';

  @override
  String get registryBuildFailed => '构建或发布失败。请检查 Dockerfile、Buildx 平台支持及仓库写入权限。';

  @override
  String get registryBuildContextError => '请选择含有 Dockerfile 的项目文件夹。';

  @override
  String get registryPin => '按摘要固定此版本';

  @override
  String get dockerKubeDeployAction => '部署';
}
