//! Encontrar una tecla en un deck con carpetas dentro de carpetas.
//!
//! En cuanto pasan de dos niveles, «donde puse esto» se convierte en abrir
//! carpetas a ciegas. El panel ensena quince teclas de una superficie; lo demas no
//! esta en pantalla.
//!
//! Todo aqui es **puro**: entra un `Deck` y sale una lista. No toca Windows ni el
//! disco, asi que se prueba entero.

use crate::model::{Action, Deck, Surface};
use serde::Serialize;

/// Una tecla encontrada, con lo necesario para llegar hasta ella.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Hallazgo {
    pub button_id: String,
    pub label: String,
    /// Por que encaja: la etiqueta no hace falta explicarla, pero «abre
    /// C:\...\informe.xlsx» si, porque no se ve en la tecla.
    pub detalle: String,
    /// Superficies desde la raiz hasta la que contiene la tecla, la ultima
    /// incluida. Es lo que el panel necesita para plantarse alli.
    pub camino: Vec<String>,
    /// El mismo camino en nombres, para una persona: «Mi Deck › SIA › Informes».
    pub ruta: String,
    /// En que pagina de esa superficie esta.
    pub page: usize,
}

/// Busca teclas cuya etiqueta o destino contenga la consulta.
///
/// Sin distinguir mayusculas ni acentos: quien busca «camara» tiene que encontrar
/// «Cámara», porque tecleando deprisa nadie pone la tilde.
pub fn buscar(deck: &Deck, consulta: &str, tope: usize) -> Vec<Hallazgo> {
    let q = normalizar(consulta);
    if q.is_empty() {
        return Vec::new();
    }

    // El panel de un perfil no cuelga de la raiz: es otro arbol. Sin recorrerlo,
    // la busqueda no encontraria nada de lo que hay en los perfiles, que es justo
    // donde mas teclas se acumulan.
    let mut raices = vec![deck.root.clone()];
    for p in &deck.profiles {
        if !raices.contains(&p.surface) {
            raices.push(p.surface.clone());
        }
    }

    let mut hallazgos = Vec::new();
    for raiz in &raices {
        recorrer(deck, raiz, &mut Vec::new(), &q, &mut hallazgos);
    }

    // Primero lo que empieza por lo escrito, luego el resto de etiquetas, y al
    // final lo que solo encaja por el destino: quien escribe «out» busca antes
    // «Outlook» que una tecla llamada «Informes» que abre una carpeta de Outlook.
    hallazgos.sort_by_key(|h| (h.rango, h.label.to_lowercase()));
    // Una misma tecla puede alcanzarse por dos caminos si una carpeta se comparte
    // entre la raiz y un perfil. Se queda el mejor, que es el primero tras ordenar.
    let mut vistos = std::collections::HashSet::new();
    hallazgos.retain(|h| vistos.insert(h.hallazgo.button_id.clone()));
    hallazgos.truncate(tope);
    hallazgos.into_iter().map(|h| h.hallazgo).collect()
}

/// Un hallazgo con su rango de ordenacion, que no sale al frontend.
struct Puntuado {
    rango: u8,
    label: String,
    hallazgo: Hallazgo,
}

fn recorrer(
    deck: &Deck,
    surface_id: &str,
    camino: &mut Vec<String>,
    q: &str,
    salida: &mut Vec<Puntuado>,
) {
    // Un deck editado a mano puede traer un ciclo. Rust lo rechaza al guardar,
    // pero aqui colgaria la busqueda, asi que se corta igual.
    if camino.iter().any(|s| s == surface_id) {
        return;
    }
    let Some(surface) = deck.surfaces.get(surface_id) else {
        return;
    };
    camino.push(surface_id.to_string());

    for (pagina, page) in surface.pages.iter().enumerate() {
        for boton in &page.buttons {
            if let Some((rango, detalle)) = encaja(&boton.label, &boton.action, q) {
                salida.push(Puntuado {
                    rango,
                    label: boton.label.clone(),
                    hallazgo: Hallazgo {
                        button_id: boton.id.clone(),
                        label: boton.label.clone(),
                        detalle,
                        camino: camino.clone(),
                        ruta: nombres(deck, camino),
                        page: pagina,
                    },
                });
            }
            if let Action::Folder { surface } = &boton.action {
                recorrer(deck, surface, camino, q, salida);
            }
        }
    }

    camino.pop();
}

/// Si una tecla encaja, con que prioridad y que ensenar como explicacion.
fn encaja(label: &str, accion: &Action, q: &str) -> Option<(u8, String)> {
    let etiqueta = normalizar(label);
    if etiqueta.starts_with(q) {
        return Some((0, String::new()));
    }
    if etiqueta.contains(q) {
        return Some((1, String::new()));
    }
    let destino = texto_de(accion);
    if normalizar(&destino).contains(q) {
        return Some((2, destino));
    }
    None
}

/// Lo que de una accion puede buscar una persona.
///
/// El `match` es exhaustivo a proposito: anadir un tipo de accion obliga a decidir
/// aqui si se puede buscar por su contenido, en vez de dejarlo fuera sin querer.
fn texto_de(accion: &Action) -> String {
    match accion {
        Action::App { target, .. } => target.clone(),
        Action::Url { target, .. } => target.clone(),
        Action::Urls { targets, .. } => targets.join(" "),
        Action::Path { target } => target.clone(),
        Action::Script { target, .. } => target.clone(),
        Action::Hotkey { keys } => keys.clone(),
        Action::Text { text } => text.clone(),
        // El identificador del comando ya es texto legible: `mic_mute`, `lock`.
        Action::System { command, .. } => format!("{command:?}"),
        // Una macro se busca por sus pasos, que es lo que hace de verdad.
        Action::Macro { steps, .. } => steps
            .iter()
            .map(|p| texto_de(&p.action))
            .collect::<Vec<_>>()
            .join(" "),
        // La carpeta ya se recorre por dentro, y su nombre es el de la superficie:
        // buscar por el identificador no le sirve a nadie.
        Action::Folder { .. } => String::new(),
        // Sin contenido que se pueda leer: es justo lo que esta version no
        // entiende.
        Action::Unknown { .. } => String::new(),
    }
}

fn nombres(deck: &Deck, camino: &[String]) -> String {
    camino
        .iter()
        .map(|id| {
            deck.surfaces
                .get(id)
                .map(|s: &Surface| s.name.clone())
                .unwrap_or_else(|| id.clone())
        })
        .collect::<Vec<_>>()
        .join(" › ")
}

/// Minusculas y sin acentos, para que la busqueda no dependa de como se teclee.
fn normalizar(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            otro => otro,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DeckButton, Icon, MacroStep, Page, Surface};
    use std::collections::HashMap;

    fn boton(id: &str, label: &str, accion: Action) -> DeckButton {
        DeckButton {
            id: id.into(),
            position: 0,
            label: label.into(),
            icon: Icon::default(),
            action: accion,
            states: vec![],
            wheel: None,
            live: None,
            extra: Default::default(),
        }
    }

    fn app(target: &str) -> Action {
        Action::App {
            target: target.into(),
            args: String::new(),
            workdir: String::new(),
            focus_if_running: false,
        }
    }

    /// Raiz con una carpeta «SIA» dentro, y otra «Informes» dentro de esa.
    fn deck_hondo() -> Deck {
        let mut surfaces = HashMap::new();
        surfaces.insert(
            "r".to_string(),
            Surface {
                name: "Mi Deck".into(),
                pages: vec![Page {
                    buttons: vec![
                        boton("b-out", "Outlook", app("outlook.exe")),
                        boton(
                            "b-sia",
                            "SIA",
                            Action::Folder {
                                surface: "s1".into(),
                            },
                        ),
                    ],
                    extra: Default::default(),
                }],
                extra: Default::default(),
            },
        );
        surfaces.insert(
            "s1".to_string(),
            Surface {
                name: "SIA".into(),
                pages: vec![Page {
                    buttons: vec![boton(
                        "b-inf",
                        "Informes",
                        Action::Folder {
                            surface: "s2".into(),
                        },
                    )],
                    extra: Default::default(),
                }],
                extra: Default::default(),
            },
        );
        surfaces.insert(
            "s2".to_string(),
            Surface {
                name: "Informes".into(),
                pages: vec![Page {
                    buttons: vec![boton(
                        "b-mes",
                        "Cámara mensual",
                        app("C:/informes/mes.xlsx"),
                    )],
                    extra: Default::default(),
                }],
                extra: Default::default(),
            },
        );
        Deck {
            version: 1,
            settings: Default::default(),
            root: "r".into(),
            surfaces,
            profiles: vec![],
            extra: Default::default(),
        }
    }

    #[test]
    fn encuentra_una_tecla_metida_dos_carpetas_adentro() {
        // Lo que motiva todo esto: esa tecla no esta en pantalla y hoy solo se
        // llega a ella abriendo carpetas a ciegas.
        let r = buscar(&deck_hondo(), "mensual", 10);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].button_id, "b-mes");
        assert_eq!(r[0].camino, vec!["r", "s1", "s2"]);
        assert_eq!(r[0].ruta, "Mi Deck › SIA › Informes");
        assert_eq!(r[0].page, 0);
    }

    #[test]
    fn buscar_sin_tilde_encuentra_lo_que_la_lleva() {
        // Nadie teclea la tilde cuando busca deprisa.
        let r = buscar(&deck_hondo(), "camara", 10);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].label, "Cámara mensual");
    }

    #[test]
    fn tambien_encuentra_por_el_destino_y_dice_cual_es() {
        // El destino no se ve en la tecla, asi que encontrarla por el obliga a
        // explicar por que aparece.
        let r = buscar(&deck_hondo(), "xlsx", 10);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].button_id, "b-mes");
        assert!(r[0].detalle.contains("mes.xlsx"), "{:?}", r[0].detalle);
    }

    #[test]
    fn lo_que_empieza_por_lo_escrito_va_primero() {
        let mut deck = deck_hondo();
        // Una tecla que solo encaja por el destino no puede adelantar a una cuya
        // etiqueta empieza igual que lo escrito.
        if let Some(s) = deck.surfaces.get_mut("r") {
            s.pages[0]
                .buttons
                .push(boton("b-otro", "Informe", app("x-out-y.exe")));
        }
        let r = buscar(&deck, "out", 10);
        assert_eq!(r[0].label, "Outlook", "{r:?}");
    }

    #[test]
    fn una_macro_se_encuentra_por_lo_que_hacen_sus_pasos() {
        let mut deck = deck_hondo();
        if let Some(s) = deck.surfaces.get_mut("r") {
            s.pages[0].buttons.push(boton(
                "b-m",
                "Arranque",
                Action::Macro {
                    steps: vec![MacroStep {
                        action: Box::new(app("bitacora.exe")),
                        delay_ms: 0,
                        extra: Default::default(),
                    }],
                },
            ));
        }
        let r = buscar(&deck, "bitacora", 10);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].button_id, "b-m");
    }

    #[test]
    fn una_consulta_vacia_no_devuelve_el_deck_entero() {
        assert!(buscar(&deck_hondo(), "", 10).is_empty());
        assert!(buscar(&deck_hondo(), "   ", 10).is_empty());
    }

    #[test]
    fn un_ciclo_de_carpetas_no_cuelga_la_busqueda() {
        // Rust rechaza los ciclos al guardar, pero un deck.json editado a mano
        // puede traerlos y aqui serian una recursion infinita.
        let mut deck = deck_hondo();
        if let Some(s) = deck.surfaces.get_mut("s2") {
            s.pages[0].buttons.push(boton(
                "b-vuelta",
                "Atras",
                Action::Folder {
                    surface: "r".into(),
                },
            ));
        }
        let r = buscar(&deck, "outlook", 10);
        assert_eq!(r.len(), 1, "la raiz no debe recorrerse dos veces");
    }

    #[test]
    fn tambien_busca_dentro_de_los_perfiles() {
        // El panel de un perfil es otro arbol: no cuelga de la raiz. Si no se
        // recorriera, la busqueda seria ciega justo donde mas teclas hay.
        let mut deck = deck_hondo();
        deck.surfaces.insert(
            "s-excel".to_string(),
            Surface {
                name: "Excel".into(),
                pages: vec![Page {
                    buttons: vec![boton("b-tdin", "Tabla dinámica", app("excel.exe"))],
                    extra: Default::default(),
                }],
                extra: Default::default(),
            },
        );
        deck.profiles.push(crate::perfiles::Profile {
            id: "p1".into(),
            surface: "s-excel".into(),
            exes: vec!["excel.exe".into()],
            enabled: true,
        });

        let r = buscar(&deck, "dinamica", 10);
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].button_id, "b-tdin");
        assert_eq!(r[0].camino, vec!["s-excel"]);
    }

    #[test]
    fn el_tope_corta_la_lista() {
        let r = buscar(&deck_hondo(), "o", 2);
        assert!(r.len() <= 2);
    }
}
