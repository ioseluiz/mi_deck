//! Copia de seguridad del deck entero, y vuelta atras.
//!
//! `intercambio` exporta **un perfil** para pasarselo a alguien, y lo dice en su
//! propia cabecera: un formato de deck completo serviria de respaldo pero no para
//! compartir. Esto es lo otro.
//!
//! El escenario no es hipotetico: te reinstalan el equipo, o te cambian de
//! maquina. `deck.json` mas la carpeta `icons\` son meses de trabajo y hoy su
//! unica copia vive en `%APPDATA%`.
//!
//! Dos decisiones que no son obvias, las dos sobre ajustes que **no** se
//! restauran aunque viajen dentro:
//!
//!   - **La posicion de la ventana.** Viene de otra pantalla, y en la de ahora
//!     puede caer fuera. Se queda la de este equipo.
//!   - **El arranque con Windows.** Eso vive en el registro, no en `deck.json`.
//!     Restaurarlo dejaria la casilla diciendo que si con el registro diciendo
//!     que no, que es peor que no restaurarlo.

use crate::model::{Deck, IconSource};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Version del formato. Si algun dia cambia, un archivo viejo se reconoce.
pub const VERSION: u32 = 1;

/// Un deck entero listo para guardarse fuera.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Respaldo {
    pub version: u32,
    /// Cuando se hizo. Esta para que se vea al abrir el archivo y al elegir entre
    /// dos copias; no se usa para decidir nada.
    #[serde(default)]
    pub fecha: String,
    /// Que version de MiDeck lo escribio, por el mismo motivo.
    #[serde(default)]
    pub mideck: String,
    /// El deck tal cual, con sus superficies, sus perfiles y sus ajustes.
    pub deck: Deck,
    /// Las imagenes que usan las teclas, por nombre de archivo y en base64.
    ///
    /// Van dentro para que el archivo baste por si solo. Es lo que lo hace pesar,
    /// y es el precio de que restaurar en una maquina limpia no deje quince
    /// teclas sin icono.
    #[serde(default)]
    pub imagenes: HashMap<String, String>,
}

/// Recoge el deck entero en una pieza suelta.
///
/// `leer_imagen` entra por parametro para poder probar esto sin tocar el disco.
pub fn empaquetar(
    deck: &Deck,
    fecha: String,
    mideck: String,
    leer_imagen: impl Fn(&str) -> Option<Vec<u8>>,
) -> Respaldo {
    let mut imagenes = HashMap::new();
    for archivo in imagenes_usadas(deck) {
        if let Some(bytes) = leer_imagen(&archivo) {
            imagenes.insert(
                archivo,
                base64::engine::general_purpose::STANDARD.encode(bytes),
            );
        }
    }

    Respaldo {
        version: VERSION,
        fecha,
        mideck,
        deck: deck.clone(),
        imagenes,
    }
}

/// Construye el deck que queda tras restaurar.
///
/// No escribe nada: devuelve el deck resultante y deja que quien llama decida
/// cuando guardarlo. `actual` entra para conservar lo que es de esta maquina y no
/// del respaldo.
///
/// `guardar_imagen` recibe los bytes ya descodificados y devuelve el nombre con
/// el que quedaron. No tiene por que ser el de origen: la biblioteca nombra por
/// contenido, asi que las referencias se reescriben con lo que diga.
pub fn restaurar(
    respaldo: Respaldo,
    actual: &Deck,
    guardar_imagen: impl Fn(&[u8]) -> Option<String>,
) -> Result<Deck, String> {
    if respaldo.version > VERSION {
        return Err(format!(
            "Este respaldo lo escribio una version mas nueva de MiDeck (formato {}, \
             esta entiende hasta el {VERSION}). Actualiza para restaurarlo.",
            respaldo.version
        ));
    }

    let mut deck = respaldo.deck;

    if !deck.surfaces.contains_key(&deck.root) {
        return Err("El respaldo no tiene panel principal: esta incompleto.".to_string());
    }

    // Las imagenes primero: hay que saber su nombre nuevo antes de reescribir las
    // teclas que las usan.
    let mut renombradas: HashMap<String, String> = HashMap::new();
    for (nombre, datos) in &respaldo.imagenes {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(datos) else {
            continue;
        };
        if let Some(nuevo) = guardar_imagen(&bytes) {
            if &nuevo != nombre {
                renombradas.insert(nombre.clone(), nuevo);
            }
        }
    }
    if !renombradas.is_empty() {
        for surface in deck.surfaces.values_mut() {
            for page in &mut surface.pages {
                for boton in &mut page.buttons {
                    if let IconSource::Image { file } = &mut boton.icon.source {
                        if let Some(nuevo) = renombradas.get(file) {
                            *file = nuevo.clone();
                        }
                    }
                }
            }
        }
    }

    // Lo que es de esta maquina y no del respaldo. Ver la cabecera del modulo.
    deck.settings.window = actual.settings.window;
    deck.settings.start_with_windows = actual.settings.start_with_windows;

    Ok(deck)
}

/// `mideck-respaldo-AAAAMMDD-HHMM.json`, en hora local.
///
/// Con la fecha dentro del nombre y no solo dentro del archivo: quien guarda tres
/// copias en una carpeta las distingue sin abrirlas.
pub fn nombre_de_archivo(y: u16, m: u16, d: u16, hh: u16, mm: u16) -> String {
    format!("mideck-respaldo-{y:04}{m:02}{d:02}-{hh:02}{mm:02}.json")
}

/// Todas las imagenes de la biblioteca que alguna tecla usa.
///
/// Solo las usadas: una imagen que ninguna tecla referencia es basura que el
/// propio MiDeck sabe listar y borrar desde Ajustes, y meterla aqui engordaria
/// cada copia con lo que ya sobra.
fn imagenes_usadas(deck: &Deck) -> Vec<String> {
    let mut v: Vec<String> = deck
        .surfaces
        .values()
        .flat_map(|s| s.pages.iter())
        .flat_map(|p| p.buttons.iter())
        .filter_map(|b| match &b.icon.source {
            IconSource::Image { file } => Some(file.clone()),
            _ => None,
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, DeckButton, Icon, Page, Surface};

    fn deck_con(botones: Vec<DeckButton>) -> Deck {
        let mut surfaces = HashMap::new();
        surfaces.insert(
            "r".to_string(),
            Surface {
                name: "Raiz".to_string(),
                pages: vec![Page {
                    buttons: botones,
                    extra: Default::default(),
                }],
                extra: Default::default(),
            },
        );
        Deck {
            version: 1,
            settings: Default::default(),
            root: "r".to_string(),
            surfaces,
            profiles: vec![],
            extra: Default::default(),
        }
    }

    fn boton_con_imagen(id: &str, archivo: &str) -> DeckButton {
        DeckButton {
            id: id.to_string(),
            position: 0,
            label: id.to_string(),
            icon: Icon {
                source: IconSource::Image {
                    file: archivo.to_string(),
                },
                ..Icon::default()
            },
            action: Action::App {
                target: "notepad".into(),
                args: String::new(),
                workdir: String::new(),
                focus_if_running: false,
            },
            states: vec![],
            wheel: None,
            live: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn el_respaldo_lleva_las_imagenes_que_usan_las_teclas() {
        let deck = deck_con(vec![boton_con_imagen("b1", "logo.png")]);
        let r = empaquetar(&deck, "hoy".into(), "0.5.0".into(), |_| Some(vec![1, 2, 3]));

        assert_eq!(r.imagenes.len(), 1);
        assert!(r.imagenes.contains_key("logo.png"));
        assert_eq!(r.deck.surfaces["r"].pages[0].buttons.len(), 1);
    }

    #[test]
    fn una_imagen_que_no_usa_nadie_no_engorda_el_respaldo() {
        let deck = deck_con(vec![]);
        let r = empaquetar(&deck, "hoy".into(), "0.5.0".into(), |_| Some(vec![1, 2, 3]));
        assert!(r.imagenes.is_empty());
    }

    #[test]
    fn restaurar_devuelve_el_deck_del_respaldo() {
        let viejo = deck_con(vec![boton_con_imagen("b1", "logo.png")]);
        let r = empaquetar(&viejo, "hoy".into(), "0.5.0".into(), |_| Some(vec![9]));

        let actual = deck_con(vec![]);
        let nuevo = restaurar(r, &actual, |_| Some("logo.png".to_string())).unwrap();

        assert_eq!(nuevo.surfaces["r"].pages[0].buttons.len(), 1);
    }

    #[test]
    fn si_la_imagen_queda_con_otro_nombre_la_tecla_apunta_al_nuevo() {
        // La biblioteca nombra por hash de contenido, asi que al restaurar en otra
        // maquina el archivo puede acabar llamandose de otra forma. Sin reescribir
        // la referencia, la tecla se quedaria sin icono y nadie sabria por que.
        let viejo = deck_con(vec![boton_con_imagen("b1", "viejo.png")]);
        let r = empaquetar(&viejo, "hoy".into(), "0.5.0".into(), |_| Some(vec![9]));

        let nuevo = restaurar(r, &deck_con(vec![]), |_| Some("nuevo.png".to_string())).unwrap();

        let IconSource::Image { file } = &nuevo.surfaces["r"].pages[0].buttons[0].icon.source
        else {
            panic!("deberia seguir siendo una imagen");
        };
        assert_eq!(file, "nuevo.png");
    }

    #[test]
    fn la_posicion_de_la_ventana_y_el_autoarranque_son_de_esta_maquina() {
        // Un respaldo hecho en un equipo de dos pantallas puede traer una posicion
        // que aqui cae fuera; y el autoarranque vive en el registro, asi que
        // restaurarlo dejaria la casilla y el registro diciendo cosas distintas.
        let mut viejo = deck_con(vec![]);
        viejo.settings.window = Some(crate::model::WindowPos { x: 3000, y: 40 });
        viejo.settings.start_with_windows = true;
        viejo.settings.key_size = 128;
        let r = empaquetar(&viejo, "hoy".into(), "0.5.0".into(), |_| None);

        let mut actual = deck_con(vec![]);
        actual.settings.window = Some(crate::model::WindowPos { x: 10, y: 10 });
        actual.settings.start_with_windows = false;

        let nuevo = restaurar(r, &actual, |_| None).unwrap();

        assert_eq!(nuevo.settings.window, actual.settings.window);
        assert!(!nuevo.settings.start_with_windows);
        // Y lo demas si viene del respaldo.
        assert_eq!(nuevo.settings.key_size, 128);
    }

    #[test]
    fn un_respaldo_de_una_version_mas_nueva_se_rechaza_entero() {
        // Mejor negarse que restaurar a medias: a medias es un deck que parece
        // bueno y no lo es.
        let mut r = empaquetar(&deck_con(vec![]), "hoy".into(), "9.9".into(), |_| None);
        r.version = VERSION + 1;

        let e = restaurar(r, &deck_con(vec![]), |_| None).unwrap_err();
        assert!(e.contains("mas nueva"), "{e}");
    }

    #[test]
    fn un_respaldo_sin_panel_principal_se_rechaza() {
        let mut r = empaquetar(&deck_con(vec![]), "hoy".into(), "0.5.0".into(), |_| None);
        r.deck.root = "no-existe".to_string();

        assert!(restaurar(r, &deck_con(vec![]), |_| None).is_err());
    }

    #[test]
    fn el_respaldo_sobrevive_a_pasar_por_el_archivo() {
        // Las dos mitades se prueban por separado arriba; esto prueba la unica que
        // importa de verdad: que lo que se escribe en disco se puede volver a
        // leer. `Deck` lleva un `flatten`, y un `flatten` dentro de otra
        // estructura es justo donde serde da sorpresas.
        let mut viejo = deck_con(vec![boton_con_imagen("b1", "logo.png")]);
        viejo.settings.key_size = 128;
        viejo
            .extra
            .insert("campo_futuro".into(), serde_json::json!({"x": 1}));

        let r = empaquetar(&viejo, "2026-10-15 09:05".into(), "0.5.0".into(), |_| {
            Some(vec![7, 7, 7])
        });

        let json = serde_json::to_string_pretty(&r).unwrap();
        let releido: Respaldo = serde_json::from_str(&json).expect("deberia poder releerse");

        assert_eq!(releido.fecha, "2026-10-15 09:05");
        assert_eq!(releido.mideck, "0.5.0");
        assert_eq!(releido.imagenes.len(), 1);

        let nuevo = restaurar(releido, &deck_con(vec![]), |_| Some("logo.png".into())).unwrap();
        assert_eq!(nuevo.settings.key_size, 128);
        assert_eq!(nuevo.surfaces["r"].pages[0].buttons[0].id, "b1");
        // Y lo que esta version no conoce tambien vuelve, que para eso esta.
        assert_eq!(nuevo.extra["campo_futuro"]["x"], 1);
    }

    #[test]
    fn el_nombre_del_archivo_lleva_la_fecha_para_poder_elegir_entre_copias() {
        assert_eq!(
            nombre_de_archivo(2026, 10, 15, 9, 5),
            "mideck-respaldo-20261015-0905.json"
        );
    }
}
