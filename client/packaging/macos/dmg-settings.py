"""Finder layout written directly by dmgbuild (no Finder automation)."""
import os

application = defines['app']  # noqa: F821 — injected by dmgbuild
terminal = defines['terminal']  # noqa: F821
files = [application, terminal]
symlinks = {'Applications': '/Applications'}
format = 'UDZO'
filesystem = 'HFS+'
background = defines['background']  # noqa: F821
# Finder uses points. The arrow in the Retina artwork is centered at (484, 359) pt.
row_y = 359
icon_locations = {
    os.path.basename(application): (128, row_y),
    os.path.basename(terminal): (334, row_y),
    'Applications': (634, row_y),
}
window_rect = ((160, 120), (768, 512))
default_view = 'icon-view'
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
show_icon_preview = False
include_icon_view_settings = True
include_list_view_settings = False
arrange_by = None
icon_size = 88
text_size = 14
label_pos = 'bottom'
# FinderInfo on a signed .app fails strict codesign verification in the DMG.
hide_extensions = []
