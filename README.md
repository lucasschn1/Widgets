# Widgets

Widgets de vidro para a área de trabalho do Windows 10 e 11: clima e sistema, "tocando agora"
(Spotify, YouTube…), GitHub e relógio Pomodoro.

## Instalar

No PowerShell (não precisa de Python nem de administrador):

```powershell
irm https://raw.githubusercontent.com/lucasschn1/Widgets/main/install.ps1 | iex
```

Ou baixe o `Widgets-Setup.exe` na página de [Releases](https://github.com/lucasschn1/Widgets/releases).
Na primeira vez o Windows pode mostrar "O Windows protegeu o computador" (o app não tem assinatura
digital paga): clique em **Mais informações → Executar assim mesmo**.

O app avisa quando sai uma versão nova e atualiza com um clique. Para desinstalar:
Configurações → Aplicativos → Aplicativos instalados → Widgets. As suas opções ficam em
`%APPDATA%\Widgets`.

## Rodar pelo código

```powershell
pip install -r requirements.txt
pythonw widget.pyw
```

## Lançar uma versão

1. Mude `__version__` em `deskwidgets/__init__.py` (ex.: `"2.1.0"`) e faça o commit.
2. `git tag v2.1.0` e `git push origin main v2.1.0`.

O GitHub Actions (`.github/workflows/release.yml`) gera o `Widgets.exe`, o `.zip` e o instalador e
publica o Release; quem já tem o app instalado recebe o aviso de atualização. Para gerar o
executável localmente: `pip install -r packaging/requirements-build.txt` e
`python packaging/build.py` (sai em `dist/`).

## Estrutura

```
widget.pyw                 só inicia o app
install.ps1                instalador de uma linha (baixa o último Release)
packaging/                 build do Widgets.exe (PyInstaller) e do instalador (Inno Setup)
.github/workflows/         Release automático a cada tag v*
deskwidgets/
  app.py                   App: cria os widgets e coordena menus, avisos, modos e atalhos
  config.py                ler/gravar a configuração e os valores padrão das opções
  paths.py                 caminhos (as opções do usuário ficam em %APPDATA%\Widgets)
  theme.py                 medidas, cores, fontes e ícones compartilhados
  util.py                  rede, datas em português, "há 5 min"
  win32.py                 API do Windows (janelas transparentes, monitores, DWM)
  backdrop.py              o "vidro": papel de parede desfocado atrás dos cartões
  tray.py                  ícone na bandeja, notificações e atalhos globais
  startup.py               iniciar com o Windows
  updates.py               procurar e instalar versões novas (GitHub Releases)
  ui/
    card.py                CardWidget: base de todo widget (tamanhos, arraste, encaixe, edição)
    drawing.py             formas reaproveitadas (anéis, play, pausa…)
    menu.py                menu de contexto de vidro
    overlay.py             guias de encaixe e dicas do mouse
    toast.py               avisos com "Desfazer" e a barra do modo edição
    panel.py               motor das janelas desenhadas com Pillow (controles, campos de texto)
    settings.py            janela de configurações (uma página por método `page_*`)
    prompt.py              pergunta de vidro e sugestões de cidade
    onboarding.py          boas-vindas em 3 passos
    gallery.py             miniaturas dos widgets e dicas de uso
  widgets/
    __init__.py            WIDGET_TYPES: a lista de widgets disponíveis
    system.py, media.py, github.py, pomodoro.py
```

## Criar um widget novo

1. Crie `deskwidgets/widgets/meu_widget.py` com uma subclasse de `CardWidget` que defina
   `key`, `title`, `description`, `icon` (nome em `theme.ICONS`), `layout(mode)`,
   `draw_shapes()`, `draw_content()` e `load_sample()` (dados de exemplo para a galeria).
2. Adicione a classe em `WIDGET_TYPES`, em `deskwidgets/widgets/__init__.py`.
3. Para ter uma página nas configurações, crie `widget_<key>(self, ui, x, y, w, widget, cfg)`
   em `ui/settings.py`, como as que já existem.

## Uma opção nova nas configurações

Coloque o valor padrão em `SETTINGS` (`config.py`), leia com `app.setting("nome")` e grave com
`app.set_setting("nome", valor)`, que já aplica na hora. Na página, use os blocos prontos:
`self.sw` (interruptor), `self.sl` (slider), `self.seg` (botões segmentados), `self.btn`,
`self.field` (texto) e `self.hk` (atalho de teclado), dentro de `self.rows(...)`.
