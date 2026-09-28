"""Finder layout written directly by dmgbuild (no Finder automation)."""
import os

application = defines['app']  # noqa: F821 — injected by dmgbuild
files = [application]
symlinks = {'Applications': '/Applications'}
format = 'UDZO'
filesystem = 'HFS+'
background = defines['background']  # noqa: F821
# Keep icons and native black Finder labels on the light installation area.
# The arrow in the 2x background is centered at approximately (384, 326).
icon_locations = {os.path.basename(application): (230, 326), 'Applications': (538, 326)}
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
icon_size = 96
text_size = 16
label_pos = 'bottom'
# FinderInfo on a signed .app fails strict codesign verification in the DMG.
hide_extensions = []
