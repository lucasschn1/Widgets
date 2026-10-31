<h1 align="center">Widgets</h1>

<p align="center">
  Widgets de vidro para a área de trabalho do Windows: clima, sistema, a música que está tocando,
  seu GitHub e um relógio Pomodoro.
</p>

<p align="center">
  <a href="https://github.com/lucasschn1/Widgets/releases/latest"><img alt="Última versão" src="https://img.shields.io/github/v/release/lucasschn1/Widgets?label=vers%C3%A3o&color=0a84ff"></a>
  <img alt="Windows 10 e 11" src="https://img.shields.io/badge/Windows-10%20%7C%2011-0a84ff">
  <a href="https://github.com/lucasschn1/Widgets/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/lucasschn1/Widgets/total?label=downloads&color=bf5af2"></a>
</p>

<p align="center">
  <img src="docs/images/desktop.png" width="760" alt="Os quatro widgets na área de trabalho">
</p>

Eles ficam presos ao papel de parede, atrás das janelas, e o fundo de cada um é o próprio
papel de parede desfocado, como um vidro fosco. Quando o mouse sai de cima, ficam um pouco
mais transparentes, para não roubar a atenção.

## Instalar

Abra o **PowerShell** e cole:

```powershell
irm https://raw.githubusercontent.com/lucasschn1/Widgets/main/install.ps1 | iex
```

Pronto. Não precisa ter Python nem ser administrador. Se preferir o jeito tradicional, baixe o
**[Widgets-Setup.exe](https://github.com/lucasschn1/Widgets/releases/latest)** e siga o
instalador.

> Na primeira vez, o Windows pode avisar que "protegeu o computador". É porque o app não tem
> uma assinatura digital paga. Clique em **Mais informações** e depois em **Executar assim mesmo**.

Quando sair uma versão nova, o próprio app avisa e atualiza com um clique. Para desinstalar,
é pelo caminho de sempre: Configurações → Aplicativos → Widgets.

## O que vem junto

**Clima e Sistema** mostra o tempo agora e nas próximas horas, além de CPU, memória e disco
em anéis. Avisa quando vai começar a chover.

**Tocando agora** acompanha o que estiver tocando no Windows (Spotify, YouTube no navegador e
outros), com capa, barra de progresso e controles. A roda do mouse em cima dele muda o volume.

**GitHub** traz o mapa de contribuições, a sequência de dias e as últimas atividades. Se você
usa o [GitHub CLI](https://cli.github.com/), entram também os repositórios privados e um aviso
quando alguém pede sua revisão.

**Pomodoro** é um relógio com timer de foco e pausas. Ele conta os ciclos até a pausa longa,
soma quanto você focou no dia e começa ou pausa de qualquer lugar com `Ctrl+Alt+P`.

<p align="center">
  <img src="docs/images/pomodoro.webp" width="300" alt="O Pomodoro rodando">
</p>

## Do tamanho que você quiser

Cada widget tem três tamanhos. Clique no botão que aparece no canto, ou dê dois cliques no
widget. Os vizinhos se ajeitam sozinhos.

<p align="center">
  <img src="docs/images/tamanhos.webp" width="340" alt="Mudando de tamanho">
</p>

<p align="center">
  <img src="docs/images/tamanhos.png" width="760" alt="Pequeno, médio e grande">
</p>

Para mover, é só arrastar: eles se encaixam uns nos outros e nas bordas da tela (segure `Alt`
para soltar livre). Parou o mouse em cima de um widget pequeno? Ele abre por um instante e
fecha quando você sai.

## Configurações

Tudo muda na hora, sem botão de salvar. Dá para ajustar o quanto o vidro desfoca e escurece
(com uma prévia ao vivo), escolher quais widgets aparecem, mudar os atalhos de teclado e
decidir quais avisos você quer receber.

<table>
  <tr>
    <td><img src="docs/images/config-aparencia.png" alt="Configurações: aparência"></td>
    <td><img src="docs/images/config-galeria.png" alt="Configurações: galeria de widgets"></td>
  </tr>
</table>

Na primeira vez que você abre, umas boas-vindas rápidas ajudam a escolher os widgets e
configurar a cidade do clima.

<p align="center">
  <img src="docs/images/boas-vindas.png" width="600" alt="Boas-vindas">
</p>

## Atalhos que valem a pena conhecer

| | |
|---|---|
| `Ctrl+Alt+W` | Traz os widgets para a frente das janelas |
| `Ctrl+Alt+P` | Começa ou pausa o Pomodoro |
| Clique direito | Menu com layouts, modo foco, modo edição e configurações |
| Modo edição | Fecha, redimensiona e move tudo de uma vez (e tem "Desfazer") |
| Modo foco | Esconde os widgets por 30 min, 1 h, 2 h ou até você desligar |

Os dois atalhos de teclado podem ser trocados em Configurações → Atalhos e dicas.

---

## Para quem quer mexer no código

Precisa de um Python 3 recente (o desenvolvimento é feito no 3.14):

```powershell
pip install -r requirements.txt
pythonw widget.pyw
```

Tudo é desenhado com o [Pillow](https://python-pillow.org/) e mostrado em janelas com
transparência por pixel. As opções ficam em `%APPDATA%\Widgets`, tanto rodando pelo código
quanto instalado.

<details>
<summary><b>Como o código está organizado</b></summary>

```
widget.pyw                 só inicia o app
install.ps1                instalador de uma linha (baixa o último Release)
packaging/                 build do Widgets.exe (PyInstaller) e do instalador (Inno Setup)
docs/make_images.py        gera as imagens deste README
deskwidgets/
  app.py                   cria os widgets e coordena menus, avisos, modos e atalhos
  config.py                lê e grava as opções, com os valores padrão
  paths.py                 onde ficam os arquivos
  theme.py                 medidas, cores, fontes e ícones
  util.py                  rede, datas em português, "há 5 min"
  win32.py                 API do Windows (janelas transparentes, monitores)
  backdrop.py              o vidro: o papel de parede desfocado atrás dos cartões
  tray.py                  ícone na bandeja, notificações e atalhos globais
  startup.py               iniciar com o Windows
  updates.py               procurar e instalar versões novas
  ui/
    card.py                a base de todo widget (tamanhos, arraste, encaixe, edição)
    panel.py               o motor das janelas desenhadas (controles e campos de texto)
    settings.py            as configurações, uma página por método `page_*`
    onboarding.py          as boas-vindas
    prompt.py              as perguntas de vidro e as sugestões de cidade
    toast.py               os avisos com "Desfazer" e a barra do modo edição
    menu.py, overlay.py, drawing.py, gallery.py
  widgets/
    __init__.py            a lista de widgets disponíveis
    system.py, media.py, github.py, pomodoro.py
```

</details>

<details>
<summary><b>Criando um widget novo</b></summary>

1. Crie um arquivo em `deskwidgets/widgets/` com uma subclasse de `CardWidget`. Ela precisa de
   `key`, `title`, `description`, `icon`, `layout(mode)`, `draw_shapes()`, `draw_content()` e
   `load_sample()`, que dá os dados de exemplo para a miniatura da galeria.
2. Coloque a classe em `WIDGET_TYPES`, no `deskwidgets/widgets/__init__.py`.
3. Se quiser uma página própria nas configurações, crie `widget_<key>()` em `ui/settings.py`,
   seguindo as que já existem.

Uma opção nova vai em `SETTINGS` (`config.py`). Leia com `app.setting("nome")` e grave com
`app.set_setting("nome", valor)`, que já aplica na hora.

</details>

<details>
<summary><b>Lançando uma versão</b></summary>

1. Mude `__version__` em `deskwidgets/__init__.py` e faça o commit.
2. Crie e envie a tag: `git tag v2.1.0` e depois `git push origin main v2.1.0`.

O GitHub Actions gera o `Widgets.exe`, o `.zip` e o instalador, e publica o Release. Quem já
tem o app instalado recebe o aviso de atualização. Para gerar o executável na sua máquina, use
`pip install -r packaging/requirements-build.txt` e depois `python packaging/build.py`.

As imagens deste README saem do próprio código, com dados de exemplo. Se o visual mudar,
rode `python docs/make_images.py`.

</details>
