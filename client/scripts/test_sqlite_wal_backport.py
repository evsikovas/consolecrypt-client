#!/usr/bin/env python3
"""Compile the actual WAL readers and exercise mismatched page sizes.

Only the file read is stubbed: the production C function must initialize every
requested byte. A reverse-patched negative control proves the test catches the
original disclosure rather than merely checking source text or a version.
"""
from pathlib import Path
import os
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
VENDOR = ROOT / 'client/rust/storage-core/vendor/libsqlite3-sys'
COMPILER = os.environ.get('CC') or next((c for c in ['cc', 'gcc', 'clang', 'cl'] if shutil.which(c)), None)
PREFIX = r'''
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#define SQLITE_PRIVATE
#define testcase(x) ((void)0)
#define WAL_FRAME_HDRSIZE 24
typedef unsigned char u8;
typedef uint32_t u32;
typedef int64_t i64;
typedef struct { struct { int szPage; } hdr; void *pWalFd; } Wal;
static i64 walFrameOffset(u32 frame, int sz) { return 32 + (frame-1)*(sz+24); }
static int sqlite3OsRead(void *file, void *out, int count, i64 offset) {
  (void)file; (void)offset;
  memset(out, 0x42, count);
  return 0;
}
'''
SUFFIX = r'''
int main(void) {
  const int sizes[] = {512, 1024, 4096, 32768, 65536};
  u8 page[65538];
  unsigned int i, j;
  int n;
  for (i=0; i<sizeof(sizes)/sizeof(sizes[0]); i++) {
    for (j=0; j<sizeof(sizes)/sizeof(sizes[0]); j++) {
      const int walSize = sizes[i], requested = sizes[j];
      Wal wal = {{walSize == 65536 ? 1 : walSize}, 0};
      memset(page, 0xa5, sizeof(page));
      if (sqlite3WalReadFrame(&wal, 2, requested, page+1) != 0) return 1;
      if (page[0] != 0xa5 || page[requested+1] != 0xa5) return 2;
      for (n=0; n<requested; n++) {
        if (page[n+1] != (n<walSize ? 0x42 : 0)) {
          fprintf(stderr, "uninitialized WAL tail: wal=%d requested=%d byte=%d\n", walSize, requested, n);
          return 3;
        }
      }
    }
  }
  return 0;
}
'''


@unittest.skipUnless(COMPILER, 'C compiler required for WAL regression')
class SQLiteWalBackportTests(unittest.TestCase):
    def compile_and_run(self, function):
        with tempfile.TemporaryDirectory(prefix='cc-sqlite-wal-') as temp:
            root = Path(temp)
            source = root / 'wal.c'
            source.write_text(PREFIX + function + SUFFIX)
            output = root / ('wal.exe' if os.name == 'nt' else 'wal')
            if Path(COMPILER).stem.lower() == 'cl':
                command = [COMPILER, '/nologo', '/O2', str(source), '/Fe:' + str(output)]
            else:
                command = [COMPILER, '-O2', '-std=c99', str(source), '-o', str(output)]
            build = subprocess.run(command, cwd=root, capture_output=True, text=True, timeout=120)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            return subprocess.run([str(output)], cwd=root, capture_output=True, text=True, timeout=15)

    def test_both_bundled_readers_zero_tail_and_negative_control_detects_original_bug(self):
        for engine in ['sqlite3', 'sqlcipher']:
            with self.subTest(engine=engine):
                source = (VENDOR / engine / 'sqlite3.c').read_text(encoding='utf-8')
                function = re.search(r'SQLITE_PRIVATE int sqlite3WalReadFrame\(\n.*?\n}\n', source, re.S).group()
                fixed = self.compile_and_run(function)
                self.assertEqual(fixed.returncode, 0, fixed.stderr)
                original = function.replace('  if( nOut>sz ){\n    memset(pOut+sz, 0, nOut-sz);\n    nOut = sz;\n  }\n', '')
                original = original.replace('pOut, nOut, iOffset);', 'pOut, (nOut>sz ? sz : nOut), iOffset);')
                self.assertNotEqual(original, function)
                negative = self.compile_and_run(original)
                self.assertEqual(negative.returncode, 3, negative.stderr)
                self.assertIn('uninitialized WAL tail', negative.stderr)


if __name__ == '__main__':
    unittest.main()
