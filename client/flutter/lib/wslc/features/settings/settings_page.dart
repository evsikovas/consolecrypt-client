import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:material_ui/material_ui.dart';

class SettingsPage extends StatefulWidget {
  const SettingsPage({super.key, required this.controller});
  final DeskController controller;
  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> {
  late final path = TextEditingController(text: widget.controller.cliPath ?? '');
  @override
  void dispose() {
    path.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    return ListView(
      children: [
        Panel(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('Подключение к WSLc', style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 12),
              const Text('Укажите установленный wslc.exe или оставьте поле пустым для стандартного каталога WSL.'),
              const SizedBox(height: 12),
              TextField(
                key: const ValueKey('wslc-cli-path'),
                controller: path,
                decoration: const InputDecoration(labelText: 'Путь к wslc.exe'),
              ),
              const SizedBox(height: 12),
              FilledButton.icon(
                onPressed: c.connecting ? null : () => c.connect(path.text.trim().isEmpty ? null : path.text.trim()),
                icon: const Icon(Icons.refresh_rounded),
                label: const Text('Проверить подключение'),
              ),
              if (c.runtime != null)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: Text('WSLc ${c.runtime!.cliVersion} · ${c.runtime!.readable ? 'доступен' : 'недоступен'}'),
                ),
              const SizedBox(height: 20),
              DropdownButtonFormField<int>(
                initialValue: c.refreshSeconds,
                decoration: const InputDecoration(labelText: 'Обновлять данные'),
                items: [
                  for (final seconds in const [2, 5, 10])
                    DropdownMenuItem(value: seconds, child: Text('Каждые $seconds сек.')),
                ],
                onChanged: (value) {
                  if (value != null) c.setRefreshSeconds(value);
                },
              ),
              const SizedBox(height: 16),
              Text('Сохранённых шаблонов запуска: ${c.templates.length}'),
              const SizedBox(height: 8),
              const Text(
                'Шаблоны хранятся в зашифрованной базе текущего профиля на этом устройстве. Оформление и очистка буфера задаются в общих настройках ConsoleCrypt.',
              ),
            ],
          ),
        ),
      ],
    );
  }
}
