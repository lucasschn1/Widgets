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

fn read_json(path: &PathBuf) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(text.trim_start_matches('\u{feff}')).ok())
        .filter(|v: &Value| v.is_object())
        .unwrap_or_else(|| json!({}))
}

pub fn load_config(widget_keys: &[&str]) -> Value {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    if cfg!(debug_assertions) && !dir.join("config.json").exists() {
        // primeira execução de teste: começa com uma cópia das opções de verdade
        if let Some(real) = dir.parent().map(|p| p.join("Widgets").join("config.json")) {
            let _ = std::fs::copy(real, dir.join("config.json"));
        }
    }
    let mut cfg = read_json(&dir.join("config.json"));
    let map = cfg.as_object_mut().unwrap();
    split_weather(map);
    for key in widget_keys {
        if !map.get(*key).is_some_and(Value::is_object) {
            map.insert(key.to_string(), json!({}));
        }
    }
    if !map.get("app").is_some_and(Value::is_object) {
        map.insert("app".into(), json!({}));
    }
    cfg
}

/// Versão 3.1: o "Clima e Sistema" virou dois widgets. O Tempo fica no lugar dele, com a cidade;
/// o Sistema vai para logo abaixo (o App acerta a posição ao abrir, pela marca "sistema_abaixo").
fn split_weather(map: &mut Map<String, Value>) {
    if map.contains_key("tempo") {
        return;
    }
    let Some(old) = map.get("sistema").and_then(Value::as_object).cloned() else { return };
    let mut tempo = Map::new();
    for k in ["city", "lat", "lon", "place", "x", "y", "tamanho", "visible"] {
        if let Some(v) = old.get(k) {
            tempo.insert(k.into(), v.clone());
        }
    }
    let mut system = Map::new();
    for k in ["tamanho", "visible"] {
        if let Some(v) = old.get(k) {
            system.insert(k.into(), v.clone());
        }
    }
    if old.contains_key("x") {
        system.insert("sistema_abaixo".into(), json!(true));
    }
    map.insert("tempo".into(), Value::Object(tempo));
    map.insert("sistema".into(), Value::Object(system));
    // layouts salvos: o Tempo fica onde o widget antigo estava
    if let Some(layouts) = map.get_mut("app").and_then(|a| a.get_mut("layouts")).and_then(Value::as_object_mut) {
        for layout in layouts.values_mut().filter_map(Value::as_object_mut) {
            if let Some(entry) = layout.remove("sistema") {
                layout.insert("tempo".into(), entry);
            }
        }
    }
}

pub fn save_config(cfg: &Value) {
    let path = data_dir().join("config.json");
    if let Ok(text) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(path, text);
    }
}

pub fn load_cache() -> Value {
    read_json(&data_dir().join("cache.json"))
}

pub fn save_cache(cache: &Value) {
    if let Ok(text) = serde_json::to_string(cache) {
        let _ = std::fs::write(data_dir().join("cache.json"), text);
    }
}

/// Valores padrão das opções gerais (cfg["app"]).
pub fn default_setting(name: &str) -> Value {
    match name {
        "desfoque" => json!(crate::theme::BLUR),
        "escurecimento" => json!(crate::theme::TINT_AMOUNT),
        "texto" => json!(0.0), // 0 = seguir o Windows
        "esmaecer" | "expandir_hover" | "atalhos_clique" | "dicas" | "economia_bateria" => json!(true),
        "opacidade_ociosa" => json!(0.8),
        "animacoes" => json!("auto"),
        "notificacoes" => json!({"cpu": true, "ram": true, "disco": true, "chuva": true, "github": true, "pomodoro": true}),
        _ => Value::Null,
    }
}

/// Opção geral, ou o padrão se o usuário nunca mudou.
pub fn setting(cfg: &Value, name: &str) -> Value {
    match cfg.get("app").and_then(|a| a.get(name)) {
        Some(v) if !v.is_null() => v.clone(),
        _ => default_setting(name),
    }
}

pub fn setting_bool(cfg: &Value, name: &str) -> bool {
    setting(cfg, name).as_bool().unwrap_or(false)
}

pub fn setting_f64(cfg: &Value, name: &str) -> f64 {
    setting(cfg, name).as_f64().unwrap_or(0.0)
}

pub fn setting_str(cfg: &Value, name: &str) -> String {
    setting(cfg, name).as_str().unwrap_or_default().to_string()
}

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
        self.get(key).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64)))
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
