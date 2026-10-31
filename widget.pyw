"""Widgets de área de trabalho: clima e sistema, "tocando agora", GitHub e relógio Pomodoro.

Arraste com o botão esquerdo para mover (eles se encaixam uns nos outros; segure Alt
para soltar livre). Clique com o botão direito, ou no ícone da bandeja, para o menu.
O visual é desenhado com Pillow e exibido em janelas com transparência por pixel; o
fundo dos cartões é "vidro": o pedaço do papel de parede atrás deles, desfocado.

Este arquivo só inicia o app; o código fica no pacote `deskwidgets` (veja o README).
"""

from deskwidgets.app import main

if __name__ == "__main__":
    main()
