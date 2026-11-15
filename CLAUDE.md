# Widgets: como trabalhar neste repositório

App de widgets para a área de trabalho do Windows, em Rust (Direct2D/DirectWrite/WinRT, sem
framework de interface). Textos, comentários, commits e PRs em português.

## Fluxo de cada alteração no código

1. **Toda alteração de código começa numa branch nova**, criada a partir da `main` atualizada:
   `git switch main && git pull --ff-only && git switch -c <tipo>/<descricao>`, com `tipo` =
   `feat`, `fix`, `refactor`, `test`, `docs` ou `ci`. Nunca altere código direto na `main`.
2. **Escreva ou ajuste os testes junto** com a mudança (veja "Testes" abaixo). Um bug corrigido
   ganha um teste que falharia sem a correção.
3. **Rode `cargo test --release`** antes de terminar a resposta.
4. **Ao terminar cada resposta, o hook `Stop` aplica a branch nos widgets instalados do usuário**
   (`dev/aplicar.ps1 -Auto`, configurado em `.claude/settings.local.json`): compila a versão
   final, roda os testes, troca o `Widgets.exe` instalado e confere se o app ficou aberto. Se a
   compilação, os testes ou a abertura falharem, o hook devolve o erro e os widgets continuam na
   versão anterior: corrija antes de encerrar. O usuário vê o resultado ao vivo na área de
   trabalho.
5. **Não faça commit nem push sem o usuário pedir.** Ele confere nos widgets e diz quando está
   bom. Aí: commit (mensagem no estilo `feat(widget): ...`), push da branch e PR com descrição.
   A `main` é protegida: o PR só entra com o check `testes` do GitHub Actions verde.
6. Depois do merge, volte para a `main` atualizada e apague a branch local.

Mudanças só de documentação (README, imagens) também vão por branch e PR, mas não disparam
recompilação (o hook olha só `src`, `res`, `Cargo.*` e `build.rs`).

Para devolver os widgets à versão lançada: `powershell -File dev\aplicar.ps1 -Voltar`.

## Testes

- `cargo test --release`: unidade e integração dentro do app (rápido, uns 10 s).
- `cargo test`: tudo, incluindo `tests/binario.rs`, que roda o `Widgets.exe` de verdade (linha de
  status do Claude Code e o app abrindo do zero num perfil temporário). É o que o CI roda.
- Onde ficam:
  - testes de unidade em cada módulo (`#[cfg(test)] mod tests`);
  - `src/testes.rs`: todos os widgets, em todos os tamanhos e estados, desenhando e respondendo
    ao mouse em toda a área do cartão; cada widget registrado em todas as listas;
  - `src/settings.rs`: todas as páginas das configurações e os passos das boas-vindas;
  - `tests/binario.rs`: o executável.
- Widget novo ou estado novo de um widget: acrescente-o em `cases()` de `src/testes.rs` (com um
  construtor `sample_*` marcado `#[cfg(test)]`, como os que já existem).
- Testes nunca tocam nas opções reais (`%APPDATA%\Widgets`), na inicialização do Windows nem no
  `~/.claude` do usuário: use pastas temporárias e as variáveis `APPDATA`, `USERPROFILE` e
  `CLAUDE_CONFIG_DIR`. A compilação final (`--release`) aberta de fora da pasta de instalação
  reescreveria a "iniciar com o Windows": não abra o app da compilação final num teste.

## Cuidados neste PC

- O **Controle Inteligente de Aplicativos** do Windows está ligado e às vezes barra um
  executável recém-compilado (sem assinatura): o `cargo test` pode falhar com "could not execute
  process ... (never executed)". Rode de novo; o `dev/aplicar.ps1` já tenta de novo sozinho e,
  se o Windows barrar o `Widgets.exe` novo, volta o anterior e avisa.
- PowerShell 5.1: scripts `.ps1` com acentos precisam ser salvos em UTF-8 **com BOM**, e nomes de
  funções e variáveis não diferenciam maiúsculas (`$marker` é o mesmo que `$Marker`).

## Desempenho

Otimize com números, não no chute:

- **App aberto, uso de verdade:** feche os widgets e abra o instalado com `WIDGETS_PERF=1`; a
  cada 10 s, `%TEMP%\widgets-perf.txt` mostra a CPU do processo e da thread da interface e, por
  widget, quanto cada etapa custa (conteúdo, vidro, quadro na tela, partes da placa de vídeo), em
  ciclos de CPU de verdade (sem contar espera). Desligado, o medidor (`src/meter.rs`) não custa nada.
- **Bancada** (`src/perf.rs`, testes ignorados): `cargo test --release perf::perf -- --ignored
  --nocapture --exact` (cada widget e o vidro) e `perf::gpu_frame` (um quadro na placa de vídeo,
  também no ritmo do app). Compare números da mesma rodada: o PC muda de velocidade com a carga.
- O que já se aprendeu: o vidro guarda o fundo e as peças (`GlassParts`) enquanto o cartão não
  anda; na placa de vídeo, imagens que se repetem por muitos quadros vão como `Kept` (enviadas uma
  vez só); um quadro na placa de vídeo tem um piso fixo (~0,5 ms de CPU no ritmo do app), então
  o que mais pesa são as animações contínuas.
- Uma mudança de desempenho não pode mudar o desenho: compare com o jeito antigo num teste (byte
  a byte quando der; no máximo 1 tom de diferença, como em `glass_parts_match_finish_glass`).

## Imagens do README

Geradas pelo código: `cargo test --release readme_images::readme -- --ignored --exact` e depois
`python packaging\readme_anim.py`. Veja a seção "Para quem quer mexer no código" do README.
