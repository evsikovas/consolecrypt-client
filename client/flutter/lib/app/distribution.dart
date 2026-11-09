/// Flutter provides this value automatically for `--flavor rustore`.
/// It is compiled into the app; store behavior never depends on user settings.
const isRuStoreDistribution = String.fromEnvironment('FLUTTER_APP_FLAVOR') == 'rustore';

final ruStoreApplicationPage = Uri.https('www.rustore.ru', '/catalog/app/io.consolecrypt.consolecrypt');
