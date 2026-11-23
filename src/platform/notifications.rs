//! As notificações dos outros apps (WhatsApp, Teams, Outlook...), lidas do Windows para a ilha.
//!
//! O Windows só deixa um app ler as notificações dos outros com a permissão da pessoa (em
//! Configurações → Privacidade → Notificações), pedida uma vez. Ler não apaga nada: a
//! notificação continua na central do Windows. O aviso de "chegou uma nova" do Windows só vale
//! para apps empacotados; aqui, um fio confere a central a cada segundo e só acorda a interface
//! quando algo mudou.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use windows::core::Interface;
use windows::Foundation::Size;
use windows::Storage::Streams::DataReader;
use windows::UI::Notifications::Management::{UserNotificationListener, UserNotificationListenerAccessStatus};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, UserNotification};
use windows_future::IAsyncOperation;

/// de quanto em quanto tempo o fio confere a central de notificações
const POLL_EVERY: Duration = Duration::from_secs(1);
/// o tamanho pedido para o ícone de cada app (em pixels): nítido mesmo grande na ilha aberta
const LOGO_PX: f32 = 128.0;

/// A permissão para ler as notificações.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Allowed,
    /// a pessoa (ou o Windows) negou
    Denied,
    /// ainda não perguntado
    Unasked,
    /// este Windows não tem o leitor de notificações
    Unavailable,
}

/// Uma notificação de outro app.
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    /// o número dela no Windows (não se repete enquanto ela existe)
    pub id: u32,
    /// o nome do app ("WhatsApp") e a identidade dele no Windows (para abrir o app e lembrar das
    /// escolhas)
    pub app: String,
    pub app_id: String,
    /// a primeira linha (quem mandou, o assunto) e o resto
    pub title: String,
    pub body: String,
    /// quando chegou, em segundos desde 1970
    pub at: f64,
}

/// O que o fio manda: as notificações na central agora e os ícones dos apps que ele ainda não
/// tinha mandado (o arquivo da imagem, para a interface abrir).
pub struct Batch {
    pub toasts: Vec<Toast>,
    pub logos: Vec<(String, Vec<u8>)>,
}

/// O fio que vigia a central: para sozinho quando este `Watch` é solto.
pub struct Watch {
    pub rx: Receiver<Batch>,
    stop: Arc<AtomicBool>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A permissão de agora (sem perguntar nada).
pub fn access() -> Access {
    let Ok(listener) = UserNotificationListener::Current() else { return Access::Unavailable };
    match listener.GetAccessStatus() {
        Ok(status) => access_of(status),
        Err(_) => Access::Unavailable,
    }
}

/// Pede a permissão (o Windows pergunta na primeira vez; depois, só devolve o que a pessoa
/// escolheu). Espera a resposta: chame fora da thread da interface.
pub fn request_access() -> Access {
    let Ok(listener) = UserNotificationListener::Current() else { return Access::Unavailable };
    match listener.RequestAccessAsync().and_then(|op| op.join()) {
        Ok(status) => access_of(status),
        Err(_) => Access::Unavailable,
    }
}

/// Pede a permissão num fio à parte (a interface não trava esperando a pessoa responder) e
/// acorda a interface quando ela responde.
pub fn request_access_async() {
    let _ = std::thread::Builder::new().name("notificacoes-permissao".into()).spawn(|| {
        unsafe {
            let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
        }
        request_access();
        crate::platform::events::wake();
    });
}

fn access_of(status: UserNotificationListenerAccessStatus) -> Access {
    match status {
        UserNotificationListenerAccessStatus::Allowed => Access::Allowed,
        UserNotificationListenerAccessStatus::Denied => Access::Denied,
        _ => Access::Unasked,
    }
}

/// Começa a vigiar a central de notificações.
pub fn watch() -> Option<Watch> {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::Builder::new().name("notificacoes".into()).spawn(move || watch_loop(tx, flag)).ok()?;
    Some(Watch { rx, stop })
}

/// O fio: lê a central a cada segundo e manda a lista só quando ela mudou (ou chegou o ícone de
/// um app novo), para não acordar a interface à toa.
fn watch_loop(tx: Sender<Batch>, stop: Arc<AtomicBool>) {
    unsafe {
        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
    }
    let mut last_ids: Option<Vec<u32>> = None;
    let mut logos_sent: HashSet<String> = HashSet::new();
    while !stop.load(Ordering::Relaxed) {
        if let Some(raw) = read_raw() {
            let mut logos = Vec::new();
            for notification in &raw {
                let Some(info) = notification.AppInfo().ok() else { continue };
                let Ok(app_id) = info.AppUserModelId().map(|s| s.to_string()) else { continue };
                if logos_sent.insert(app_id.clone()) {
                    if let Some(bytes) = logo_bytes(&info) {
                        logos.push((app_id, bytes));
                    }
                }
            }
            let toasts: Vec<Toast> = raw.iter().filter_map(toast_of).collect();
            let mut ids: Vec<u32> = toasts.iter().map(|t| t.id).collect();
            ids.sort_unstable();
            if last_ids.as_ref() != Some(&ids) || !logos.is_empty() {
                if tx.send(Batch { toasts, logos }).is_err() {
                    break;
                }
                crate::platform::events::wake();
                last_ids = Some(ids);
            }
        }
        std::thread::sleep(POLL_EVERY);
    }
}

/// As notificações na central agora, como o Windows as dá (None sem a permissão).
fn read_raw() -> Option<Vec<UserNotification>> {
    let listener = UserNotificationListener::Current().ok()?;
    let list = listener.GetNotificationsAsync(NotificationKinds::Toast).ok()?.join().ok()?;
    let count = list.Size().ok()?;
    Some((0..count).filter_map(|i| list.GetAt(i).ok()).collect())
}

/// As notificações na central agora (None sem a permissão ou se o Windows não respondeu).
#[cfg(test)]
pub fn read() -> Option<Vec<Toast>> {
    Some(read_raw()?.iter().filter_map(toast_of).collect())
}

/// O ícone do app, como arquivo de imagem (None se o app não tem).
fn logo_bytes(info: &windows::ApplicationModel::AppInfo) -> Option<Vec<u8>> {
    let logo = info.DisplayInfo().ok()?.GetLogo(Size { Width: LOGO_PX, Height: LOGO_PX }).ok()?;
    let stream = logo.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()? as u32;
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let op: IAsyncOperation<u32> = reader.LoadAsync(size).ok()?.cast().ok()?;
    op.join().ok()?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    Some(bytes)
}

/// O que importa de uma notificação do Windows (None se não dá para ler).
fn toast_of(notification: &UserNotification) -> Option<Toast> {
    let id = notification.Id().ok()?;
    let info = notification.AppInfo().ok();
    let app = info.as_ref().and_then(|i| i.DisplayInfo().ok()).and_then(|d| d.DisplayName().ok()).map(|s| s.to_string()).unwrap_or_default();
    let app_id = info.as_ref().and_then(|i| i.AppUserModelId().ok()).map(|s| s.to_string()).unwrap_or_default();
    let binding = notification.Notification().ok()?.Visual().ok()?.GetBinding(&KnownNotificationBindings::ToastGeneric().ok()?).ok()?;
    let texts = binding.GetTextElements().ok()?;
    let lines: Vec<String> = (0..texts.Size().unwrap_or(0)).filter_map(|i| texts.GetAt(i).ok()).filter_map(|t| t.Text().ok()).map(|s| s.to_string()).filter(|s| !s.trim().is_empty()).collect();
    let (title, body) = split_lines(&lines);
    // DateTime: intervalos de 100 ns desde 1601
    let ticks = notification.CreationTime().ok()?.UniversalTime;
    let at = ticks as f64 / 1e7 - 11_644_473_600.0;
    Some(Toast { id, app, app_id, title, body, at })
}

/// As linhas de texto de uma notificação: a primeira é o título (quem mandou, o assunto) e as
/// outras, juntas, o corpo.
fn split_lines(lines: &[String]) -> (String, String) {
    let title = lines.first().cloned().unwrap_or_default();
    let body = lines.get(1..).map(|rest| rest.join(" · ")).unwrap_or_default();
    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_line_is_the_title() {
        let lines = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(split_lines(&lines(&["Maria", "chego em 10 min"])), ("Maria".into(), "chego em 10 min".into()));
        assert_eq!(split_lines(&lines(&["Reunião", "Sala 3", "às 15h"])), ("Reunião".into(), "Sala 3 · às 15h".into()));
        assert_eq!(split_lines(&lines(&["Só o título"])), ("Só o título".into(), String::new()));
        assert_eq!(split_lines(&[]), (String::new(), String::new()));
    }

    /// Confere, no PC de verdade, se o Windows deixa este app ler as notificações (só a
    /// permissão e quantas há; nada do que elas dizem):
    /// cargo test --release notifications_probe -- --ignored --nocapture
    #[test]
    #[ignore]
    fn notifications_probe() {
        println!("permissão: {:?}", access());
        if access() == Access::Allowed {
            let list = read();
            println!("notificações lidas: {:?}", list.as_ref().map(Vec::len));
            println!("com app e título: {:?}", list.as_ref().map(|l| l.iter().filter(|t| !t.app.is_empty() && !t.title.is_empty()).count()));
            // os apps (sem nada do que dizem) e se o texto veio
            for t in list.unwrap_or_default() {
                println!("  {} ({}) · título: {} · corpo: {}", t.app, t.app_id, !t.title.is_empty(), !t.body.is_empty());
            }
        }
    }
}
