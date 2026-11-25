//! Configuração (config.json) e cache (cache.json), no mesmo formato da versão em Python.
//!
//! Tudo fica num `serde_json::Value`: assim as chaves que esta versão ainda não conhece
//! (ou que vieram de outra) são preservadas ao gravar.

use std::path::PathBuf;

use serde_json::{json, Map, Value};

/// Pasta das opções do usuário: %APPDATA%\Widgets (ou Widgets-dev nas compilações de teste,
/// para não mexer nas configurações de verdade enquanto o app é desenvolvido).
pub fn data_dir() -> PathBuf {
    let base = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    let name = if cfg!(debug_assertions) { "Widgets-dev" } else { "Widgets" };
    base.join(name)
}

/// Um objeto JSON lido de `path` (sem o arquivo, ou se ele não lê: um objeto vazio).
fn read_json(path: &PathBuf) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(text.trim_start_matches('\u{feff}')).ok())
        .filter(|value: &Value| value.is_object())
        .unwrap_or_else(|| json!({}))
}

/// Lê o config.json, já com as migrações das versões antigas e uma seção para cada widget.
pub fn load_config(widget_keys: &[&str]) -> Value {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    if cfg!(debug_assertions) && !dir.join("config.json").exists() {
        // primeira execução de teste: começa com uma cópia das opções de verdade
        if let Some(real) = dir.parent().map(|parent| parent.join("Widgets").join("config.json")) {
            let _ = std::fs::copy(real, dir.join("config.json"));
        }
    }
    let mut cfg = read_json(&dir.join("config.json"));
    let map = cfg.as_object_mut().unwrap();
    // instalação nova: ainda não há nada guardado (as opções e posições nascem dos padrões)
    let existing = !map.is_empty();
    split_weather(map);
    add_sections(map, widget_keys, existing);
    if !map.get("app").is_some_and(Value::is_object) {
        map.insert("app".into(), json!({}));
    }
    keep_old_defaults(map, existing);
    cfg
}

/// Uma seção para cada widget. Um widget que chegou numa versão nova não aparece sozinho para
/// quem já usava o app (fica oculto, na galeria).
fn add_sections(map: &mut Map<String, Value>, widget_keys: &[&str], existing: bool) {
    for key in widget_keys {
        if !map.get(*key).is_some_and(Value::is_object) {
            let added = existing && crate::widgets::ADDED_LATER.contains(key);
            map.insert(key.to_string(), if added { json!({"visible": false}) } else { json!({}) });
        }
    }
}

/// Versão dos padrões das opções (ver `default_setting` e widgets::DEFAULT_MODE).
const DEFAULTS_VERSION: i64 = 2;

/// Versão 3.7: os padrões mudaram (vidro com 12 px de desfoque e 57% de escurecimento, sem
/// esmaecer, animações sempre ligadas e todos os widgets no tamanho médio). Uma opção que o
/// usuário deixou no padrão não fica gravada, então quem já usava o app passaria a ver os padrões
/// novos sem ter mudado nada: para essas instalações, os padrões antigos são gravados como
/// escolhas, uma vez só. Instalações novas ficam com os padrões novos.
fn keep_old_defaults(map: &mut Map<String, Value>, existing: bool) {
    let app = map.get_mut("app").and_then(Value::as_object_mut).unwrap();
    if app.get("padroes").and_then(Value::as_i64).unwrap_or(0) >= DEFAULTS_VERSION {
        return;
    }
    app.insert("padroes".into(), json!(DEFAULTS_VERSION));
    if !existing {
        return;
    }
    let old = [("desfoque", json!(26.0)), ("escurecimento", json!(0.40)), ("animacoes", json!("auto"))];
    for (name, value) in old {
        if app.get(name).is_none_or(Value::is_null) {
            app.insert(name.into(), value);
        }
    }
    // até a 3.6, só o Tempo, o Sistema e o Pomodoro nasciam no tamanho médio
    for key in ["midia", "github"] {
        if let Some(section) = map.get_mut(key).and_then(Value::as_object_mut) {
            if !section.contains_key("tamanho") && !section.contains_key("compact") {
                section.insert("tamanho".into(), json!("g"));
            }
        }
    }
}

/// Versão 3.1: o "Clima e Sistema" virou dois widgets. O Tempo fica no lugar dele, com a cidade;
/// o Sistema vai para logo abaixo (o App acerta a posição ao abrir, pela marca "sistema_abaixo").
fn split_weather(map: &mut Map<String, Value>) {
    if map.contains_key("tempo") {
        return;
    }
    let Some(old) = map.get("sistema").and_then(Value::as_object).cloned() else { return };
    let mut tempo = Map::new();
    for key in ["city", "lat", "lon", "place", "x", "y", "tamanho", "visible"] {
        if let Some(value) = old.get(key) {
            tempo.insert(key.into(), value.clone());
        }
    }
    let mut system = Map::new();
    for key in ["tamanho", "visible"] {
        if let Some(value) = old.get(key) {
            system.insert(key.into(), value.clone());
        }
    }
    if old.contains_key("x") {
        system.insert("sistema_abaixo".into(), json!(true));
    }
    map.insert("tempo".into(), Value::Object(tempo));
    map.insert("sistema".into(), Value::Object(system));
    // layouts salvos: o Tempo fica onde o widget antigo estava
    if let Some(layouts) = map.get_mut("app").and_then(|app| app.get_mut("layouts")).and_then(Value::as_object_mut) {
        for layout in layouts.values_mut().filter_map(Value::as_object_mut) {
            if let Some(entry) = layout.remove("sistema") {
                layout.insert("tempo".into(), entry);
            }
        }
    }
}

/// Grava o config.json.
pub fn save_config(cfg: &Value) {
    let path = data_dir().join("config.json");
    if let Ok(text) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(path, text);
    }
}

/// Lê o cache.json (os últimos dados de cada widget).
pub fn load_cache() -> Value {
    read_json(&data_dir().join("cache.json"))
}

/// Grava o cache.json.
pub fn save_cache(cache: &Value) {
    if let Ok(text) = serde_json::to_string(cache) {
        let _ = std::fs::write(data_dir().join("cache.json"), text);
    }
}

/// Valores padrão das opções gerais (cfg["app"]): os de uma instalação nova.
pub fn default_setting(name: &str) -> Value {
    match name {
        "desfoque" => json!(crate::ui::theme::BLUR),
        "escurecimento" => json!(crate::ui::theme::TINT_AMOUNT),
        "texto" => json!(0.0), // 0 = seguir o Windows
        crate::data::shared::WALL_COLOR => json!(false),
        crate::data::shared::LIGHT_THEME => json!(true),
        "atalhos_clique" | "dicas" | "economia_bateria" | "procurar_atualizacoes" | "ilha_widgets" => json!(true),
        // (experimental: desligado até ser conferido em mais PCs)
        "placa_de_video" => json!(false),
        "animacoes" => json!("sempre"),
        "notificacoes" => json!({"cpu": true, "ram": true, "disco": true, "chuva": true, "github": true, "pomodoro": true, "bateria": true}),
        _ => Value::Null,
    }
}

/// Opção geral, ou o padrão se o usuário nunca mudou.
pub fn setting(cfg: &Value, name: &str) -> Value {
    match cfg.get("app").and_then(|app| app.get(name)) {
        Some(value) if !value.is_null() => value.clone(),
        _ => default_setting(name),
    }
}

/// `setting` como sim ou não.
pub fn setting_bool(cfg: &Value, name: &str) -> bool {
    setting(cfg, name).as_bool().unwrap_or(false)
}

/// `setting` como número.
pub fn setting_f64(cfg: &Value, name: &str) -> f64 {
    setting(cfg, name).as_f64().unwrap_or(0.0)
}

/// `setting` como texto.
pub fn setting_str(cfg: &Value, name: &str) -> String {
    setting(cfg, name).as_str().unwrap_or_default().to_string()
}

/// Limites dos avisos, os mesmos na notificação, na ilha e no texto das Configurações:
/// CPU ou memória acima de `HIGH_USE` por `HIGH_USE_SECS` segundos; bateria abaixo de `LOW_BATTERY`%.
pub const HIGH_USE: f32 = 0.9;
pub const HIGH_USE_SECS: u64 = 180;
pub const LOW_BATTERY: u8 = 15;

/// "90%"
pub fn high_use_pct() -> String {
    format!("{:.0}%", HIGH_USE * 100.0)
}

/// "3 minutos" (ou "1 minuto", "45 segundos")
pub fn high_use_time() -> String {
    match HIGH_USE_SECS {
        60 => "1 minuto".into(),
        secs if secs % 60 == 0 => format!("{} minutos", secs / 60),
        secs => format!("{secs} segundos"),
    }
}

/// A notificação `kind` ("cpu", "chuva"...) está ligada nas Configurações?
pub fn notify_enabled(cfg: &Value, kind: &str) -> bool {
    let user = setting(cfg, "notificacoes");
    user.get(kind).and_then(Value::as_bool).or_else(|| default_setting("notificacoes").get(kind).and_then(Value::as_bool)).unwrap_or(false)
}

/// Atalhos para ler valores de um objeto JSON.
pub trait Get {
    fn str_of(&self, key: &str) -> Option<&str>;
    fn f64_of(&self, key: &str) -> Option<f64>;
    fn i64_of(&self, key: &str) -> Option<i64>;
    fn bool_of(&self, key: &str) -> Option<bool>;
    fn obj_mut(&mut self, key: &str) -> &mut Map<String, Value>;
}

impl Get for Value {
    fn str_of(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }
    fn f64_of(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Value::as_f64)
    }
    fn i64_of(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|float| float.round() as i64)))
    }
    fn bool_of(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(Value::as_bool)
    }
    /// O objeto em `key` (criado vazio se não existir).
    fn obj_mut(&mut self, key: &str) -> &mut Map<String, Value> {
        let map = self.as_object_mut().expect("objeto JSON");
        if !map.get(key).is_some_and(Value::is_object) {
            map.insert(key.to_string(), json!({}));
        }
        map.get_mut(key).unwrap().as_object_mut().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(text: &str) -> Value {
        let mut cfg: Value = serde_json::from_str(text).unwrap();
        let map = cfg.as_object_mut().unwrap();
        let existing = !map.is_empty();
        split_weather(map);
        add_sections(map, crate::widgets::KEYS, existing);
        map.entry("app").or_insert(json!({}));
        keep_old_defaults(map, existing);
        cfg
    }

    #[test]
    fn new_install_gets_new_defaults() {
        let cfg = loaded("{}");
        assert_eq!(setting_f64(&cfg, "desfoque"), 12.0);
        assert_eq!(setting_f64(&cfg, "escurecimento"), 0.57);
        assert_eq!(setting_str(&cfg, "animacoes"), "sempre");
        // nada gravado além da marca: os padrões continuam vindo do código
        assert_eq!(cfg["app"], json!({"padroes": DEFAULTS_VERSION}));
        for key in crate::widgets::KEYS {
            assert_eq!(cfg[*key], json!({}), "{key}");
        }
    }

    #[test]
    fn existing_install_keeps_old_look() {
        // quem nunca mexeu nas opções (nada gravado) e tem o GitHub e o Tocando agora no
        // tamanho de antes (grande, sem "tamanho" gravado)
        let cfg = loaded(r#"{"tempo": {"x": 10, "y": 20}, "midia": {"x": 10, "y": 300}, "github": {}, "app": {"dicas_vistas": true}}"#);
        assert_eq!(setting_f64(&cfg, "desfoque"), 26.0);
        assert_eq!(setting_f64(&cfg, "escurecimento"), 0.40);
        assert_eq!(setting_str(&cfg, "animacoes"), "auto");
        assert_eq!(cfg["midia"]["tamanho"], "g");
        assert_eq!(cfg["github"]["tamanho"], "g");
        assert!(cfg["tempo"].get("tamanho").is_none(), "o Tempo já nascia médio");
        // o Uso do Claude chegou depois: fica oculto para quem já usava
        assert_eq!(cfg["claude"]["visible"], false);
        assert_eq!(cfg["tempo"]["x"], 10);
    }

    #[test]
    fn personal_choices_are_untouched() {
        let text = r#"{"app": {"desfoque": 18, "escurecimento": 0.3, "animacoes": "nunca", "texto": 1.1},
                       "midia": {"tamanho": "p", "ilha": true}, "github": {"compact": true, "usuario": "alguem"},
                       "tempo": {"city": "Curitiba", "tamanho": "g"}}"#;
        let before: Value = serde_json::from_str(text).unwrap();
        let cfg = loaded(text);
        for name in ["desfoque", "escurecimento", "animacoes", "texto"] {
            assert_eq!(cfg["app"][name], before["app"][name], "{name}");
        }
        assert_eq!(cfg["midia"], before["midia"]);
        assert_eq!(cfg["github"], before["github"], "o \"compact\" antigo ainda decide o tamanho");
        assert_eq!(cfg["tempo"], before["tempo"]);
    }

    #[test]
    fn old_defaults_are_kept_only_once() {
        let mut cfg = loaded(r#"{"app": {}}"#);
        // depois da migração, o usuário volta uma opção para o padrão novo (que não é gravado)
        cfg["app"].as_object_mut().unwrap().remove("desfoque");
        let map = cfg.as_object_mut().unwrap();
        keep_old_defaults(map, true);
        assert_eq!(setting_f64(&cfg, "desfoque"), 12.0);
    }
}
