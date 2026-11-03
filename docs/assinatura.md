# Assinatura digital pela SignPath Foundation

Sem assinatura, o **Controle Inteligente de Aplicativos** do Windows 11 pode barrar cada versão
nova do `Widgets.exe` até a Microsoft "conhecer" o arquivo. Um programa com assinatura digital
válida passa. A [SignPath Foundation](https://signpath.org) assina projetos de código aberto de
graça. Em troca, o editor que aparece no Windows é "SignPath Foundation", e cada versão precisa
da sua aprovação manual.

O repositório já está pronto para isso:

- `README.md` tem a seção **Code signing policy** (exigida), com os papéis e a privacidade;
- o app tem opções para desligar o que ele manda para a internet sem você pedir (capa em alta do
  YouTube e busca automática de atualizações), e o instalador e o `install.ps1` mostram o resumo
  de privacidade antes de instalar (exigido);
- `.github/workflows/release.yml` assina o `Widgets.exe` e o instalador quando a SignPath estiver
  configurada; sem ela, os lançamentos continuam saindo sem assinatura;
- `.signpath/` tem as duas configurações de artefato para colar no site da SignPath.

## 1. Antes de pedir

1. **Autenticação em dois passos no GitHub** (exigida para quem mexe no repositório):
   <https://github.com/settings/security>.
2. **Lance uma versão com estas mudanças** (o projeto precisa já estar lançado na forma que vai
   ser assinada, com a seção Code signing policy no README).

## 2. Pedir

Preencha o formulário em <https://signpath.org/apply> com os dados do projeto: o repositório
(`https://github.com/lucasschn1/Widgets`), a licença (MIT) e o link para a seção
[Code signing policy](../README.md#code-signing-policy). A aprovação é feita por pessoas da
SignPath e pode levar alguns dias.

## 3. Depois de aprovado, no site da SignPath

1. Ligue a autenticação em dois passos na sua conta da SignPath.
2. No projeto (o *slug* esperado é `Widgets`), ligue o *Trusted Build System* **GitHub.com**.
3. Crie duas *Artifact Configurations*, colando o conteúdo dos arquivos:
   - slug `widgets-exe`: [`.signpath/widgets-exe.xml`](../.signpath/widgets-exe.xml);
   - slug `widgets-setup`: [`.signpath/widgets-setup.xml`](../.signpath/widgets-setup.xml).
4. Confira a *Signing Policy* (o slug esperado é `release-signing`), com o certificado da SignPath
   Foundation e você como aprovador (*Approver*).
5. Crie um usuário de CI e gere um *API token* para ele, com permissão de enviar pedidos nesse
   projeto.
6. Anote o *Organization ID* (aparece no site da SignPath).

## 4. No GitHub

Em **Settings → Secrets and variables → Actions** do repositório:

| Tipo | Nome | Valor |
|---|---|---|
| Secret | `SIGNPATH_API_TOKEN` | o API token do passo 3.5 |
| Variable | `SIGNPATH_ORGANIZATION_ID` | o Organization ID |
| Variable (opcional) | `SIGNPATH_PROJECT_SLUG` | só se o projeto não se chamar `Widgets` |
| Variable (opcional) | `SIGNPATH_SIGNING_POLICY_SLUG` | só se a política não se chamar `release-signing` |

A variável `SIGNPATH_ORGANIZATION_ID` é o que liga a assinatura no workflow.

## 5. Testar e lançar

1. Rode o workflow à mão: **Actions → Release → Run workflow**. Ele para duas vezes esperando
   aprovação (primeiro o `Widgets.exe`, depois o instalador, que leva o `.exe` já assinado dentro).
   Aprove os dois pedidos no site da SignPath; cada um espera até 1 hora.
2. Baixe o artefato gerado e confira: botão direito no `Widgets.exe` → Propriedades →
   Assinaturas Digitais, ou `Get-AuthenticodeSignature .\Widgets.exe` no PowerShell.
3. A partir daí, cada tag `v*` gera um Release assinado, sempre com as duas aprovações.

Depois que as versões estiverem saindo assinadas, vale atualizar os textos que ainda falam da
falta de assinatura: o aviso do SmartScreen no README e o corpo do Release em `release.yml`.
