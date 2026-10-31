"""Os widgets disponíveis. Para criar um novo: uma subclasse de CardWidget num módulo
desta pasta (com `key`, `title`, `description`, `icon`, `layout`, `draw_shapes`,
`draw_content` e `load_sample`) e uma linha em WIDGET_TYPES.
"""

from deskwidgets.widgets.github import GitHubWidget
from deskwidgets.widgets.media import MediaWidget
from deskwidgets.widgets.pomodoro import PomodoroWidget
from deskwidgets.widgets.system import SystemWidget

# a ordem é a dos menus, da galeria e das páginas nas configurações
WIDGET_TYPES = {cls.key: cls for cls in (SystemWidget, MediaWidget, GitHubWidget, PomodoroWidget)}
