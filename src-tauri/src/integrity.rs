//! Integridad del grafo de superficies.
//!
//! El registro plano con referencias es comodo de editar, pero trae tres fallos
//! propios que hay que atajar antes de que lleguen a la interfaz:
//!   1. Referencia rota: un boton folder apunta a una superficie que no existe.
//!   2. Ciclo: una carpeta que vuelve a un ancestro. Sin esto, navegar seria un
//!      bucle infinito.
//!   3. Huerfana: ninguna tecla la referencia, tipicamente tras borrar un boton.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::{Action, Deck};

#[derive(Debug, Default, serde::Serialize)]
pub struct Report {
    pub broken: Vec<BrokenRef>,
    /// Cada ciclo se reporta como la secuencia de superficies que lo forma.
    pub cycles: Vec<Vec<String>>,
    /// Superficies no alcanzables desde la raiz.
    pub orphans: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct BrokenRef {
    pub button_id: String,
    pub in_surface: String,
    pub missing_surface: String,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.broken.is_empty() && self.cycles.is_empty() && self.orphans.is_empty()
    }
}

/// Mapa superficie -> superficies a las que apunta por botones de tipo folder.
fn adjacency(deck: &Deck) -> HashMap<&str, Vec<&str>> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for (id, surface) in &deck.surfaces {
        let entry = adj.entry(id.as_str()).or_default();
        for page in &surface.pages {
            for button in &page.buttons {
                if let Action::Folder { surface: target } = &button.action {
                    entry.push(target.as_str());
                }
            }
        }
    }
    adj
}

pub fn check(deck: &Deck) -> Report {
    let mut report = Report::default();
    let adj = adjacency(deck);

    // 1. Referencias rotas.
    for (id, surface) in &deck.surfaces {
        for page in &surface.pages {
            for button in &page.buttons {
                if let Action::Folder { surface: target } = &button.action {
                    if !deck.surfaces.contains_key(target) {
                        report.broken.push(BrokenRef {
                            button_id: button.id.clone(),
                            in_surface: id.clone(),
                            missing_surface: target.clone(),
                        });
                    }
                }
            }
        }
    }

    // 2. Ciclos, por recorrido en profundidad con marcas de visita.
    //    gris = en la pila actual, negro = ya cerrado.
    let mut gris: HashSet<&str> = HashSet::new();
    let mut negro: HashSet<&str> = HashSet::new();
    let mut pila: Vec<&str> = Vec::new();
    let mut ids: Vec<&str> = deck.surfaces.keys().map(|s| s.as_str()).collect();
    ids.sort_unstable(); // orden estable para que el reporte sea reproducible

    for id in ids {
        if !negro.contains(id) {
            buscar_ciclos(id, &adj, &mut gris, &mut negro, &mut pila, &mut report);
        }
    }

    // 3. Huerfanas: lo no alcanzable desde la raiz.
    let alcanzables = reachable(deck, &adj);
    let mut orphans: Vec<String> = deck
        .surfaces
        .keys()
        .filter(|id| !alcanzables.contains(id.as_str()))
        .cloned()
        .collect();
    orphans.sort();
    report.orphans = orphans;

    report
}

fn buscar_ciclos<'a>(
    nodo: &'a str,
    adj: &HashMap<&'a str, Vec<&'a str>>,
    gris: &mut HashSet<&'a str>,
    negro: &mut HashSet<&'a str>,
    pila: &mut Vec<&'a str>,
    report: &mut Report,
) {
    gris.insert(nodo);
    pila.push(nodo);

    if let Some(vecinos) = adj.get(nodo) {
        for &vecino in vecinos {
            if gris.contains(vecino) {
                // Arista hacia atras: el ciclo es el tramo de la pila desde el vecino.
                let inicio = pila.iter().position(|n| *n == vecino).unwrap_or(0);
                let ciclo: Vec<String> = pila[inicio..].iter().map(|s| s.to_string()).collect();
                if !report.cycles.contains(&ciclo) {
                    report.cycles.push(ciclo);
                }
            } else if !negro.contains(vecino) && adj.contains_key(vecino) {
                buscar_ciclos(vecino, adj, gris, negro, pila, report);
            }
        }
    }

    pila.pop();
    gris.remove(nodo);
    negro.insert(nodo);
}

/// Superficies alcanzables desde cualquier punto de entrada, en anchura.
///
/// Los puntos de entrada son la raiz **y la superficie de cada perfil**. Sembrar
/// tambien desde los perfiles no es un detalle: un perfil no cuelga de la raiz
/// por definicion, asi que sin esto saldria como huerfano y el boton "Borrar
/// carpetas sin usar..." de Ajustes se llevaria todos los perfiles del usuario
/// sin preguntar. Arreglandolo aqui se corrigen de golpe los tres sitios que
/// consumen este informe: ese boton, el aviso del arranque y el contador de
/// Ajustes.
fn reachable<'a>(deck: &'a Deck, adj: &HashMap<&'a str, Vec<&'a str>>) -> HashSet<&'a str> {
    let mut vistos: HashSet<&str> = HashSet::new();
    let mut cola: VecDeque<&str> = VecDeque::new();

    if deck.surfaces.contains_key(&deck.root) {
        cola.push_back(deck.root.as_str());
        vistos.insert(deck.root.as_str());
    }
    for perfil in &deck.profiles {
        // Tambien los deshabilitados: apagar un perfil no es tirar sus teclas.
        let id = perfil.surface.as_str();
        if deck.surfaces.contains_key(id) && vistos.insert(id) {
            cola.push_back(id);
        }
    }

    while let Some(actual) = cola.pop_front() {
        if let Some(vecinos) = adj.get(actual) {
            for &vecino in vecinos {
                if deck.surfaces.contains_key(vecino) && vistos.insert(vecino) {
                    cola.push_back(vecino);
                }
            }
        }
    }
    vistos
}

/// Si poner dentro de `padre` una tecla que lleva a `hijo` crearia un ciclo.
/// Se consulta antes de crear o mover una carpeta, para rechazar la operacion
/// en vez de dejar el deck en un estado que cuelga la navegacion.
pub fn would_create_cycle(deck: &Deck, padre: &str, hijo: &str) -> bool {
    if padre == hijo {
        return true;
    }
    let adj = adjacency(deck);
    // Hay ciclo si el padre ya es alcanzable bajando desde el hijo.
    let mut vistos: HashSet<&str> = HashSet::new();
    let mut cola: VecDeque<&str> = VecDeque::new();
    cola.push_back(hijo);
    vistos.insert(hijo);

    while let Some(actual) = cola.pop_front() {
        if actual == padre {
            return true;
        }
        if let Some(vecinos) = adj.get(actual) {
            for &vecino in vecinos {
                if vistos.insert(vecino) {
                    cola.push_back(vecino);
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DeckButton, Icon, Page, Settings, Surface};
    use std::collections::HashMap;

    fn perfil(surface: &str) -> crate::perfiles::Profile {
        crate::perfiles::Profile {
            id: "p1".into(),
            surface: surface.into(),
            exes: vec!["excel.exe".into()],
            enabled: true,
        }
    }

    /// El test que mas protege de toda la fase.
    ///
    /// Un perfil no cuelga de la raiz por definicion. Si contara como huerfano,
    /// el boton "Borrar carpetas sin usar..." de Ajustes se llevaria todos los
    /// perfiles del usuario sin preguntar.
    #[test]
    fn la_superficie_de_un_perfil_no_es_huerfana() {
        let mut deck = deck_con(&[], &["s-root", "s-excel"]);
        deck.profiles = vec![perfil("s-excel")];

        let r = check(&deck);
        assert!(
            r.orphans.is_empty(),
            "el perfil salio como huerfano: {:?}",
            r.orphans
        );
    }

    #[test]
    fn purgar_huerfanas_no_se_lleva_los_perfiles() {
        let mut deck = deck_con(&[], &["s-root", "s-excel", "s-suelta"]);
        deck.profiles = vec![perfil("s-excel")];

        let borradas = crate::edit::purge_orphans(&mut deck);

        assert_eq!(borradas, vec!["s-suelta"], "deberia borrar solo la suelta");
        assert!(deck.surfaces.contains_key("s-excel"), "borro el perfil");
        assert!(deck.surfaces.contains_key("s-root"));
    }

    /// Las carpetas que cuelgan de un perfil tampoco: el perfil es un punto de
    /// entrada completo, con su arbol detras.
    #[test]
    fn lo_que_cuelga_de_un_perfil_tampoco_es_huerfano() {
        let mut deck = deck_con(&[("s-excel", "s-sub")], &["s-root", "s-excel", "s-sub"]);
        deck.profiles = vec![perfil("s-excel")];

        assert!(check(&deck).orphans.is_empty());
    }

    #[test]
    fn un_perfil_que_apunta_a_una_superficie_que_ya_no_existe_no_rompe_nada() {
        let mut deck = deck_con(&[], &["s-root"]);
        deck.profiles = vec![perfil("s-borrada")];

        // No debe entrar en panico ni inventarse superficies.
        let r = check(&deck);
        assert!(r.orphans.is_empty());
        assert!(r.broken.is_empty());
    }

    /// Construye un deck a partir de aristas (superficie -> superficie).
    fn deck_con(aristas: &[(&str, &str)], superficies: &[&str]) -> Deck {
        let mut surfaces: HashMap<String, Surface> = HashMap::new();
        for id in superficies {
            surfaces.insert(id.to_string(), Surface::new(*id));
        }
        for (i, (from, to)) in aristas.iter().enumerate() {
            let s = surfaces.get_mut(*from).expect("superficie origen");
            if s.pages.is_empty() {
                s.pages.push(Page::default());
            }
            s.pages[0].buttons.push(DeckButton {
                id: format!("b{i}"),
                position: i as u32,
                label: to.to_string(),
                icon: Icon::default(),
                action: Action::Folder {
                    surface: to.to_string(),
                },
                wheel: None,
                live: None,
                extra: Default::default(),
            });
        }
        Deck {
            version: 1,
            settings: Settings::default(),
            root: "s-root".to_string(),
            profiles: Vec::new(),
            extra: serde_json::Map::new(),
            surfaces,
        }
    }

    #[test]
    fn un_deck_sano_no_reporta_nada() {
        let deck = deck_con(
            &[("s-root", "s-a"), ("s-a", "s-b")],
            &["s-root", "s-a", "s-b"],
        );
        let r = check(&deck);
        assert!(r.is_clean(), "{r:?}");
    }

    #[test]
    fn detecta_referencia_rota() {
        let deck = deck_con(&[("s-root", "s-fantasma")], &["s-root"]);
        let r = check(&deck);
        assert_eq!(r.broken.len(), 1);
        assert_eq!(r.broken[0].missing_surface, "s-fantasma");
        assert_eq!(r.broken[0].in_surface, "s-root");
    }

    #[test]
    fn detecta_ciclo_directo() {
        let deck = deck_con(&[("s-root", "s-a"), ("s-a", "s-a")], &["s-root", "s-a"]);
        let r = check(&deck);
        assert_eq!(r.cycles.len(), 1, "{r:?}");
        assert_eq!(r.cycles[0], vec!["s-a"]);
    }

    #[test]
    fn detecta_ciclo_indirecto() {
        let deck = deck_con(
            &[("s-root", "s-a"), ("s-a", "s-b"), ("s-b", "s-a")],
            &["s-root", "s-a", "s-b"],
        );
        let r = check(&deck);
        assert_eq!(r.cycles.len(), 1, "{r:?}");
        assert_eq!(r.cycles[0], vec!["s-a", "s-b"]);
    }

    #[test]
    fn detecta_superficie_huerfana() {
        // s-suelta existe pero nadie la referencia.
        let deck = deck_con(&[("s-root", "s-a")], &["s-root", "s-a", "s-suelta"]);
        let r = check(&deck);
        assert_eq!(r.orphans, vec!["s-suelta"]);
        assert!(r.broken.is_empty());
        assert!(r.cycles.is_empty());
    }

    #[test]
    fn el_hijo_bajo_su_propio_ancestro_seria_ciclo() {
        let deck = deck_con(
            &[("s-root", "s-a"), ("s-a", "s-b")],
            &["s-root", "s-a", "s-b"],
        );
        // Meter s-a dentro de s-b cierra el lazo root -> a -> b -> a.
        assert!(would_create_cycle(&deck, "s-b", "s-a"));
        // Una carpeta dentro de si misma, tambien.
        assert!(would_create_cycle(&deck, "s-a", "s-a"));
        // Pero s-b dentro de la raiz es perfectamente legitimo.
        assert!(!would_create_cycle(&deck, "s-root", "s-b"));
    }
}
