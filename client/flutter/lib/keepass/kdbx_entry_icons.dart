import 'package:flutter/foundation.dart';
import 'package:kdbx/kdbx.dart';
import 'package:material_ui/material_ui.dart';

// The index is the standard IconID stored by KeePass.
const kdbxEntryIcons = <KdbxIcon, IconData>{
  KdbxIcon.Key: Icons.key_outlined,
  KdbxIcon.LockOpen: Icons.lock_open_outlined,
  KdbxIcon.Identity: Icons.badge_outlined,
  KdbxIcon.World: Icons.public_outlined,
  KdbxIcon.EMail: Icons.mail_outline,
  KdbxIcon.NetworkServer: Icons.dns_outlined,
  KdbxIcon.Monitor: Icons.desktop_windows_outlined,
  KdbxIcon.Console: Icons.terminal_outlined,
  KdbxIcon.Homebanking: Icons.account_balance_outlined,
  KdbxIcon.Certificate: Icons.verified_user_outlined,
  KdbxIcon.Book: Icons.menu_book_outlined,
  KdbxIcon.Star: Icons.star_outline,
  KdbxIcon.Folder: Icons.folder_outlined,
};

IconData kdbxEntryIcon(int? index, {IconData fallback = Icons.key_outlined}) {
  if (index == null || index < 0 || index >= KdbxIcon.values.length) return fallback;
  return kdbxEntryIcons[KdbxIcon.values[index]] ?? fallback;
}

Widget kdbxIconPreview(Uint8List? image, IconData fallback, {required double size}) => image == null
    ? Icon(fallback, size: size)
    : Image.memory(
        image,
        width: size,
        height: size,
        fit: BoxFit.contain,
        errorBuilder: (_, _, _) => Icon(fallback, size: size),
      );
