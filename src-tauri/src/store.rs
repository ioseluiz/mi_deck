//! Carga y guardado de deck.json.
//!
//! Dos invariantes que no se negocian:
//!   - El guardado es atomico (escribir a .tmp y renombrar). Si el archivo acaba en
//!     una carpeta sincronizada, una escritura parcial lo corromperia.
//!   - Un archivo corrupto nunca se pierde: se renombra a .bak-<fecha> y se arranca
//!     con un deck por defecto.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::{Action, Deck, DeckButton, Icon, IconSource, Page, Settings, Shell, Surface};

pub const ROOT_ID: &str = "s-root";

/// Resultado de cargar: ademas del deck, que paso al cargarlo.
pub struct LoadOutcome {
    pub deck: Deck,
    /// Ruta del respaldo, si hubo que recuperar de un archivo ilegible.
    pub recovered_from: Option<PathBuf>,
    pub warnings: Vec<String>,
}

/// Carga deck.json. Nunca falla: si el archivo no existe devuelve el deck por
/// defecto, y si esta corrupto lo respalda y devuelve el deck por defecto.
pub fn load(path: &Path) -> LoadOutcome {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return LoadOutcome {
                deck: default_deck(),
                recovered_from: None,
                warnings: Vec::new(),
            };
        }
        Err(err) => {
            return LoadOutcome {
                deck: default_deck(),
                recovered_from: None,
                warnings: vec![format!("No se pudo leer {}: {err}", path.display())],
            };
        }
    };

    match serde_json::from_str::<Deck>(&raw) {
        Ok(mut deck) => {
            migrar(&mut deck, &raw);
            LoadOutcome {
                deck,
                recovered_from: None,
                warnings: Vec::new(),
            }
        }
        Err(err) => {
            let backup = backup_path(path);
            let moved = fs::rename(path, &backup).is_ok();
            LoadOutcome {
                deck: default_deck(),
                recovered_from: moved.then(|| backup.clone()),
                warnings: vec![format!(
                    "deck.json no se pudo interpretar ({err}). Se respaldo en {} y se \
                     arranco con un deck nuevo.",
                    backup.display()
                )],
            }
        }
    }
}

/// Guardado atomico: escribe a un temporal en la misma carpeta y renombra encima.
pub fn save(deck: &Deck, path: &Path) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(deck)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json)?;
    // En Windows, fs::rename reemplaza el destino existente.
    fs::rename(&tmp, path)
}

/// Ajustes de versiones anteriores que ya no existen como tales.
///
/// El nivel de ventana era un booleano `always_on_top`. Al convertirse en los
/// tres niveles de `window_level`, un deck antiguo con el booleano en false
/// habria quedado en el nivel por defecto (encima de todo), que es justo lo
/// contrario de lo que el usuario habia elegido.
fn migrar(deck: &mut Deck, raw: &str) {
    let Ok(valor) = serde_json::from_str::<serde_json::Value>(raw) else {
        return;
    };
    let ajustes = &valor["settings"];
    if ajustes.get("window_level").is_some() {
        return; // ya viene con el campo nuevo
    }
    if let Some(encima) = ajustes.get("always_on_top").and_then(|v| v.as_bool()) {
        deck.settings.window_level = if encima {
            crate::model::WindowLevel::Top
        } else {
            crate::model::WindowLevel::Normal
        };
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let stamp = timestamp();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "deck.json".to_string());
    path.with_file_name(format!("{name}.bak-{stamp}"))
}

/// AAAAMMDD-HHMMSS en UTC, sin dependencias de fecha.
fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d, hh, mm, ss) = civil_from_epoch(secs);
    format!("{y:04}{m:02}{d:02}-{hh:02}{mm:02}{ss:02}")
}

/// Algoritmo civil-from-days de Howard Hinnant, para no arrastrar una dependencia
/// de fechas solo por nombrar un archivo de respaldo.
fn civil_from_epoch(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };

    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    )
}

// ------------------------------------------------------------ deck por defecto

/// Deck de arranque. No se deja vacio a proposito: con teclas de ejemplo, incluida
/// una carpeta con contenido, el primer arranque ya permite probar la navegacion.
pub fn default_deck() -> Deck {
    let mut surfaces = HashMap::new();

    surfaces.insert(
        ROOT_ID.to_string(),
        Surface {
            name: "Mi Deck".to_string(),
            pages: vec![Page {
                buttons: vec![
                    DeckButton {
                        id: "b-explorador".into(),
                        position: 0,
                        label: "Explorador".into(),
                        icon: Icon::default(),
                        action: Action::Path {
                            target: "%USERPROFILE%".into(),
                        },
                    },
                    DeckButton {
                        id: "b-notepad".into(),
                        position: 1,
                        label: "Bloc de notas".into(),
                        icon: Icon::default(),
                        action: Action::App {
                            target: "notepad.exe".into(),
                            args: String::new(),
                            workdir: String::new(),
                            focus_if_running: false,
                        },
                    },
                    DeckButton {
                        id: "b-ejemplos".into(),
                        position: 3,
                        label: "Ejemplos".into(),
                        icon: Icon {
                            source: IconSource::Builtin {
                                name: "folder".into(),
                            },
                            ..Icon::default()
                        },
                        action: Action::Folder {
                            surface: "s-ejemplos".into(),
                        },
                    },
                ],
            }],
        },
    );

    surfaces.insert(
        "s-ejemplos".to_string(),
        Surface {
            name: "Ejemplos".to_string(),
            pages: vec![Page {
                buttons: vec![
                    DeckButton {
                        id: "b-acp".into(),
                        position: 1,
                        label: "Canal".into(),
                        icon: Icon::default(),
                        action: Action::Url {
                            target: "https://pancanal.com".into(),
                            browser: "default".into(),
                            profile: None,
                        },
                    },
                    DeckButton {
                        id: "b-descargas".into(),
                        position: 2,
                        label: "Descargas".into(),
                        icon: Icon {
                            source: IconSource::Emoji {
                                glyph: "\u{1F4E5}".into(),
                            },
                            ..Icon::default()
                        },
                        action: Action::Path {
                            target: "%USERPROFILE%\\Downloads".into(),
                        },
                    },
                    DeckButton {
                        id: "b-version".into(),
                        position: 3,
                        label: "Versión PS".into(),
                        icon: Icon {
                            source: IconSource::Emoji {
                                glyph: "\u{1F4BB}".into(),
                            },
                            ..Icon::default()
                        },
                        action: Action::Script {
                            shell: Shell::Powershell,
                            target: "$PSVersionTable".into(),
                            args: String::new(),
                            hidden: false,
                        },
                    },
                ],
            }],
        },
    );

    Deck {
        version: crate::model::SCHEMA_VERSION,
        settings: Settings::default(),
        root: ROOT_ID.to_string(),
        surfaces,
    }
}

// ------------------------------------------------------------------- ubicacion

/// Ruta de deck.json. Por defecto %APPDATA%\MiDeck\deck.json, siguiendo la
/// convencion de no guardar datos de usuario junto al ejecutable. Se puede
/// redirigir con DECK_CONFIG, por ejemplo a una copia en OneDrive.
pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("DECK_CONFIG") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join("MiDeck").join("deck.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::atomic::{AtomicU32, Ordering};

    static CONTADOR: AtomicU32 = AtomicU32::new(0);

    fn temp_dir(tag: &str) -> PathBuf {
        let n = CONTADOR.fetch_add(1, Ordering::SeqCst);
        let dir = env::temp_dir().join(format!("mideck-test-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ida_y_vuelta_conserva_el_deck() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("deck.json");

        let original = default_deck();
        save(&original, &path).unwrap();
        let cargado = load(&path);

        assert!(cargado.warnings.is_empty());
        assert_eq!(cargado.deck.root, original.root);
        assert_eq!(cargado.deck.surfaces.len(), original.surfaces.len());
        assert_eq!(cargado.deck.settings.grid.cells(), 15);
    }

    #[test]
    fn archivo_inexistente_devuelve_deck_por_defecto() {
        let dir = temp_dir("missing");
        let salida = load(&dir.join("no-existe.json"));
        assert!(salida.warnings.is_empty());
        assert!(salida.recovered_from.is_none());
        assert_eq!(salida.deck.root, ROOT_ID);
    }

    #[test]
    fn archivo_corrupto_se_respalda_y_no_se_pierde() {
        let dir = temp_dir("corrupt");
        let path = dir.join("deck.json");
        fs::write(&path, "{ esto no es json valido ").unwrap();

        let salida = load(&path);
        let backup = salida.recovered_from.expect("deberia haber respaldo");

        assert!(backup.exists(), "el respaldo debe existir en disco");
        assert!(!path.exists(), "el original corrupto se movio");
        assert!(fs::read_to_string(&backup)
            .unwrap()
            .contains("esto no es json"));
        assert_eq!(salida.deck.root, ROOT_ID);
        assert_eq!(salida.warnings.len(), 1);
    }

    #[test]
    fn un_deck_antiguo_conserva_el_nivel_de_ventana_que_tenia() {
        use crate::model::WindowLevel;
        let dir = temp_dir("migracion");
        let path = dir.join("deck.json");

        // Formato viejo: el nivel era un booleano.
        fs::write(
            &path,
            r#"{ "root": "s-root", "settings": { "always_on_top": false },
                 "surfaces": { "s-root": { "name": "X" } } }"#,
        )
        .unwrap();
        assert_eq!(load(&path).deck.settings.window_level, WindowLevel::Normal);

        fs::write(
            &path,
            r#"{ "root": "s-root", "settings": { "always_on_top": true },
                 "surfaces": { "s-root": { "name": "X" } } }"#,
        )
        .unwrap();
        assert_eq!(load(&path).deck.settings.window_level, WindowLevel::Top);
    }

    #[test]
    fn el_campo_nuevo_tiene_prioridad_sobre_el_viejo() {
        use crate::model::WindowLevel;
        let dir = temp_dir("migracion2");
        let path = dir.join("deck.json");
        fs::write(
            &path,
            r#"{ "root": "s-root",
                 "settings": { "always_on_top": true, "window_level": "desktop" },
                 "surfaces": { "s-root": { "name": "X" } } }"#,
        )
        .unwrap();
        assert_eq!(load(&path).deck.settings.window_level, WindowLevel::Desktop);
    }

    #[test]
    fn un_deck_pasado_por_powershell_sigue_siendo_legible() {
        // ConvertTo-Json de PowerShell serializa una lista de un elemento como el
        // elemento suelto, sin corchetes. Antes esto daba el deck por corrupto y
        // se perdia el trabajo del usuario, que acababa con el deck por defecto.
        let dir = temp_dir("powershell");
        let path = dir.join("deck.json");
        fs::write(
            &path,
            r#"{
              "root": "s-root",
              "surfaces": {
                "s-root": {
                  "name": "Mi Deck",
                  "pages": {
                    "buttons": {
                      "id": "b-uno",
                      "position": 0,
                      "label": "Lista Master",
                      "icon": { "type": "auto" },
                      "action": { "type": "url", "target": "https://ejemplo" }
                    }
                  }
                }
              }
            }"#,
        )
        .unwrap();

        let salida = load(&path);
        assert!(
            salida.recovered_from.is_none(),
            "no deberia haberse dado por corrupto: {:?}",
            salida.warnings
        );
        let raiz = &salida.deck.surfaces["s-root"];
        assert_eq!(raiz.pages.len(), 1);
        assert_eq!(raiz.pages[0].buttons.len(), 1);
        assert_eq!(raiz.pages[0].buttons[0].label, "Lista Master");
    }

    #[test]
    fn el_formato_normal_con_listas_sigue_funcionando() {
        let dir = temp_dir("listas");
        let path = dir.join("deck.json");
        fs::write(
            &path,
            r#"{
              "root": "s-root",
              "surfaces": {
                "s-root": {
                  "name": "Mi Deck",
                  "pages": [
                    { "buttons": [] },
                    { "buttons": [] }
                  ]
                }
              }
            }"#,
        )
        .unwrap();
        assert_eq!(load(&path).deck.surfaces["s-root"].pages.len(), 2);
    }

    #[test]
    fn campos_faltantes_toman_valores_por_defecto() {
        let dir = temp_dir("defaults");
        let path = dir.join("deck.json");
        // Deck minimo: sin settings, sin version, superficie sin paginas.
        fs::write(
            &path,
            r#"{ "root": "s-root", "surfaces": { "s-root": { "name": "X" } } }"#,
        )
        .unwrap();

        let deck = load(&path).deck;
        assert_eq!(deck.version, crate::model::SCHEMA_VERSION);
        assert_eq!(deck.settings.grid.cols, 5);
        assert_eq!(deck.settings.grid.rows, 3);
        assert_eq!(deck.settings.key_size, 96);
        assert_eq!(deck.settings.window_level, crate::model::WindowLevel::Top);
        assert!(deck.surfaces["s-root"].pages.is_empty());
    }

    #[test]
    fn el_guardado_no_deja_temporales() {
        let dir = temp_dir("tmp");
        let path = dir.join("deck.json");
        save(&default_deck(), &path).unwrap();

        let sobrantes: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(sobrantes.is_empty(), "quedaron temporales: {sobrantes:?}");
    }

    #[test]
    fn la_fecha_del_respaldo_es_legible() {
        // 2021-01-01 00:00:00 UTC
        assert_eq!(civil_from_epoch(1_609_459_200), (2021, 1, 1, 0, 0, 0));
        // 2026-10-07 15:45:30 UTC
        assert_eq!(civil_from_epoch(1_791_387_930), (2026, 10, 7, 15, 45, 30));
        // Un anio bisiesto, que es donde el algoritmo se rompe si esta mal: 2024-02-29.
        assert_eq!(civil_from_epoch(1_709_164_800), (2024, 2, 29, 0, 0, 0));
    }
}
