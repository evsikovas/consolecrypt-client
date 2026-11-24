import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/docker/docker_kube_plan.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class RegistryCredentialDialog extends ConsumerStatefulWidget {
  const RegistryCredentialDialog({super.key});
  @override
  ConsumerState<RegistryCredentialDialog> createState() => _RegistryDialogState();
}

class _RegistryDialogState extends ConsumerState<RegistryCredentialDialog> {
  late final DockerScope scope;

  @override
  void initState() {
    super.initState();
    scope = ref.read(dockerScopeProvider);
  }

  static const presets = {
    _dockerHub: 'docker.io',
    'Harbor': '',
    'GitLab': 'registry.gitlab.com',
    'Forgejo': '',
    'GitHub': 'ghcr.io',
    'custom': '',
  };
  static const _dockerHub = 'Docker Hub'; // l10n-ignore: Official registry service name.
  String provider = _dockerHub;
  final address = TextEditingController(text: 'docker.io'),
      user = TextEditingController(),
      token = TextEditingController(),
      name = TextEditingController();
  bool busy = false;
  String? error;
  @override
  void dispose() {
    for (final c in [address, user, token, name]) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> save() async {
    if (busy || !scope.unlocked || !identical(scope, ref.read(dockerScopeProvider))) return;
    final host = address.text
        .trim()
        .toLowerCase()
        .replaceFirst(RegExp('^https://'), '')
        .replaceFirst(RegExp(r'/+$'), '');
    if (!DockerKubeTarget.validImage('$host/group/image:v1') || host.contains('/') || host.contains('@')) {
      setState(() => error = context.l10n.registryInvalid);
      return;
    }
    setState(() {
      busy = true;
      error = null;
    });
    final password = SecretText(token.text);
    try {
      final credential = await ref
          .read(inventoryServiceProvider)
          .createPasswordCredential(
            name: name.text.trim().isEmpty ? '${user.text.trim()}@$host' : name.text.trim(),
            username: user.text.trim(),
            password: password,
            registry: host,
          );
      if (mounted && identical(scope, ref.read(dockerScopeProvider))) {
        token.clear();
        closeDialog(context, credential);
      }
    } catch (_) {
      if (mounted) setState(() => error = context.l10n.registrySaveFailed);
    } finally {
      password.wipe();
      if (mounted) setState(() => busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    ref.listen(dockerScopeProvider, (_, next) {
      if (!identical(next, scope)) {
        token.clear();
        final route = ModalRoute.of(context);
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (route?.isActive == true) route!.navigator?.removeRoute(route);
        });
      }
    });
    Widget field(String key, TextEditingController controller, String label, {String? hint}) => TextField(
      key: ValueKey(key),
      controller: controller,
      enabled: !busy,
      autocorrect: false,
      decoration: InputDecoration(labelText: label, hintText: hint),
      onChanged: (_) => setState(() {}),
    );
    return GlassDialog(
      title: l.registryAdd,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          spacing: 10,
          children: [
            DropdownButtonFormField<String>(
              initialValue: provider,
              decoration: InputDecoration(labelText: l.aiProviderDialogProviderLabel),
              isExpanded: true,
              items: [
                for (final p in presets.keys)
                  DropdownMenuItem(value: p, child: Text(p == 'custom' ? l.registryOther : p)),
              ],
              onChanged: busy
                  ? null
                  : (value) {
                      if (value == null) return;
                      setState(() {
                        provider = value;
                        address.text = presets[value]!;
                        error = null;
                      });
                    },
            ),
            field('registry-address', address, l.registryAddress, hint: 'registry.example.org'),
            field('registry-name', name, l.commonName),
            field('registry-user', user, l.credentialsDetailsUsername),
            SecretField(
              key: const ValueKey('registry-token'),
              controller: token,
              enabled: !busy,
              label: l.registryToken,
            ),
            if (error != null) Text(error!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
          ],
        ),
      ),
      secondaryActions: [
        GlassButton(label: l.commonCancel, onPressed: busy ? null : () => closeDialog<Credential>(context)),
      ],
      primaryAction: ValueListenableBuilder<TextEditingValue>(
        valueListenable: token,
        builder: (_, value, _) => GlassButton.prominent(
          key: const ValueKey('registry-save'),
          label: l.commonSave,
          busy: busy,
          onPressed: busy || user.text.trim().isEmpty || value.text.isEmpty || address.text.trim().isEmpty
              ? null
              : save,
        ),
      ),
    );
  }
}
