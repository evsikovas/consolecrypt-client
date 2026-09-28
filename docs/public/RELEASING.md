# Выпуск версии

Исходники и lock-файлы хранятся в Git; DMG, APK, ZIP и установщики — только
в GitLab Releases / Package Registry или вложениях релиза. Не добавляйте `dist/`.

1. Для изменения функциональности увеличьте версию:
   `python3 client/scripts/bump-version.py --part patch` (Windows: `python`).
2. Выполните проверки и сборки скриптами `client/scripts/build-*`.
   Каждый запуск дополнительно увеличивает build number; не используйте номер повторно.
3. Проверьте пакет на целевой ОС. Сверяйте точную версию с `ConsoleCrypt.version`.
4. Зафиксируйте исходники и создайте тег, например `v0.1.2`.
5. В GitLab откройте **Deploy → Releases → New release**, выберите тег,
   добавьте описание, реальные файлы, их версии и SHA-256. Можно прикрепить файлы
   через редактор описания и использовать полученные ссылки в Release assets.
   Отсутствующий пакет платформы обозначьте явно — не создавайте неработающие ссылки.

При настроенном [GitLab CLI](https://gitlab.com/gitlab-org/cli) файлы можно загрузить
в Package Registry, не помещая их в историю исходников:

```sh
glab auth login --hostname git.evsikov.net
glab release create v0.1.2 --name 'ConsoleCrypt 0.1.2 · Preview' --notes-file release-notes.txt --repo https://git.evsikov.net/publics/consolecrypt
glab release upload v0.1.2 dist/macos/ConsoleCrypt.dmg dist/android/ConsoleCrypt-android-arm64.apk --use-package-registry --repo https://git.evsikov.net/publics/consolecrypt
```

После сборки на Windows добавьте установщик в тот же релиз (PowerShell):

```powershell
$setup = Get-ChildItem dist\windows\*-setup.exe | Sort-Object LastWriteTime -Descending | Select-Object -First 1
glab release upload v0.1.2 $setup.FullName "$($setup.FullName).sha256" --use-package-registry --repo https://git.evsikov.net/publics/consolecrypt
```

Доступ CLI нужен для API GitLab; одного SSH-ключа для загрузки бинарных файлов
недостаточно. Не сохраняйте токены, Android keystore, Apple-сертификаты или ключ
подписи Windows в Git. Для распространения за пределами preview настройте
стабильные release-подписи и нотариализацию macOS.

`.gitlab-ci.yml` содержит ручную задачу Windows. Для неё нужен настроенный Windows
runner с тегом `windows` и установленными инструментами из BUILD_WINDOWS.md.
Задача не запускается автоматически и не подменяет проверку на реальной Windows.
