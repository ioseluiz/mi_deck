//! Mutaciones del deck: crear, editar, mover y borrar teclas y carpetas.
//!
//! Son funciones puras sobre `Deck`, sin Tauri ni disco de por medio, para que se
//! puedan probar sin arrancar la interfaz. Los comandos de lib.rs solo bloquean el
//! estado, llaman aqui y guardan.
//!
//! Dos reglas que atraviesan todo el modulo:
//!   - Nada se borra en cascada. Al eliminar una tecla de carpeta, su superficie
//!     queda huerfana a proposito: un error de edicion no debe llevarse 20 botones.
//!   - Ninguna operacion puede dejar un ciclo de carpetas, que colgaria la
//!     navegacion. Se rechaza antes de tocar el deck.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::integrity;
use crate::model::{Action, Deck, DeckButton, Icon, Page, Surface};

/// Genera un id unico dentro del deck.
pub fn nuevo_id(deck: &Deck, prefijo: &str) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);

    let existentes: Vec<&str> = deck
        .surfaces
        .values()
        .flat_map(|s| s.pages.iter())
        .flat_map(|p| p.buttons.iter())
        .map(|b| b.id.as_str())
        .collect();

    loop {
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let id = format!("{prefijo}-{:x}{:02x}", (t & 0xffff_ffff) as u32, n & 0xff);
        if !existentes.contains(&id.as_str()) && !deck.surfaces.contains_key(&id) {
            return id;
        }
    }
}

/// Celdas por pagina segun los ajustes.
fn celdas(deck: &Deck) -> u32 {
    deck.settings.grid.cells().max(1)
}

/// Asegura que la superficie tiene al menos `pagina + 1` paginas.
fn asegurar_pagina(deck: &mut Deck, surface_id: &str, pagina: usize) -> Result<(), String> {
    let s = deck
        .surfaces
        .get_mut(surface_id)
        .ok_or_else(|| format!("No existe la superficie {surface_id}"))?;
    while s.pages.len() <= pagina {
        s.pages.push(Page::default());
    }
    Ok(())
}

/// Localiza un boton: (superficie, pagina, indice en el vector).
fn localizar(deck: &Deck, button_id: &str) -> Option<(String, usize, usize)> {
    for (sid, surface) in &deck.surfaces {
        for (pi, page) in surface.pages.iter().enumerate() {
            if let Some(bi) = page.buttons.iter().position(|b| b.id == button_id) {
                return Some((sid.clone(), pi, bi));
            }
        }
    }
    None
}

/// Si la celda 0 de esa superficie esta reservada a la tecla de volver.
///
/// Lo esta en toda superficie que no sea un nivel superior. Son niveles
/// superiores la raiz y las superficies de los perfiles: a un perfil no se llega
/// entrando desde ningun sitio, asi que no hay nada a lo que volver y bloquearle
/// la celda 0 solo dejaria un hueco inutilizable.
fn celda_cero_reservada(deck: &Deck, surface_id: &str) -> bool {
    !crate::perfiles::es_base(&deck.root, &deck.profiles, surface_id)
}

/// Primera celda libre de una pagina, respetando la celda 0 reservada a la tecla
/// de volver.
fn primera_libre(deck: &Deck, surface_id: &str, pagina: usize) -> Option<u32> {
    let total = celdas(deck);
    let reservada = celda_cero_reservada(deck, surface_id);
    let ocupadas: Vec<u32> = deck
        .surfaces
        .get(surface_id)
        .and_then(|s| s.pages.get(pagina))
        .map(|p| p.buttons.iter().map(|b| b.position).collect())
        .unwrap_or_default();

    (0..total).find(|i| !(reservada && *i == 0) && !ocupadas.contains(i))
}

// ------------------------------------------------------------------- escritura

/// Crea o reemplaza un boton. Si la celda pedida esta ocupada por otro boton, los
/// dos intercambian posicion: es predecible y se deshace repitiendo el gesto.
pub fn upsert_button(
    deck: &mut Deck,
    surface_id: &str,
    pagina: usize,
    mut boton: DeckButton,
) -> Result<(), String> {
    // Una tecla nueva llega sin id: lo pone el backend, que es quien puede
    // garantizar que no choque con ninguno existente. Si se dejara al frontend,
    // dos teclas creadas seguidas compartirian id y la segunda pisaria a la
    // primera en cuanto se intentara editarla o moverla.
    if boton.id.trim().is_empty() {
        boton.id = nuevo_id(deck, "b");
    }

    if let Action::Folder { surface } = &boton.action {
        if !deck.surfaces.contains_key(surface) {
            return Err(format!(
                "La carpeta apunta a una superficie inexistente: {surface}"
            ));
        }
        // La etiqueta de la tecla y el nombre de la superficie son la misma cosa
        // para quien usa el deck. Si no se sincronizan, renombrar la tecla deja
        // el nombre viejo en las migas de pan y en el desplegable de carpetas.
        // Si dos teclas apuntaran a la misma superficie, manda la ultima editada.
        let etiqueta = boton.label.trim().to_string();
        if !etiqueta.is_empty() {
            if let Some(s) = deck.surfaces.get_mut(surface) {
                s.name = etiqueta;
            }
        }
        if integrity::would_create_cycle(deck, surface_id, surface) {
            return Err(
                "Esa carpeta crearia una navegacion circular: no se puede meter dentro de \
                 si misma ni de una de sus subcarpetas."
                    .to_string(),
            );
        }
    }

    asegurar_pagina(deck, surface_id, pagina)?;

    // Si ya existia, se retira anotando de donde venia. Esa celda queda libre y
    // es la que recibira al ocupante del destino en un intercambio.
    let origen: Option<(String, usize, u32)> = localizar(deck, &boton.id).map(|(sid, pi, bi)| {
        let pos = deck.surfaces[&sid].pages[pi].buttons[bi].position;
        deck.surfaces.get_mut(&sid).unwrap().pages[pi]
            .buttons
            .remove(bi);
        (sid, pi, pos)
    });

    // Devuelve el boton a su sitio. Se usa si algo falla a partir de aqui, para no
    // perderlo por el camino.
    let restaurar = |deck: &mut Deck, boton: DeckButton, origen: &Option<(String, usize, u32)>| {
        if let Some((sid, pi, pos)) = origen {
            let mut b = boton;
            b.position = *pos;
            if let Some(s) = deck.surfaces.get_mut(sid) {
                if let Some(p) = s.pages.get_mut(*pi) {
                    p.buttons.push(b);
                }
            }
        }
    };

    // Normalizar la celda pedida: fuera de rango, o la celda 0 de una subcarpeta
    // (reservada a la tecla de volver), caen al primer hueco libre.
    let total = celdas(deck);
    let reservada = celda_cero_reservada(deck, surface_id);
    if boton.position >= total || (reservada && boton.position == 0) {
        match primera_libre(deck, surface_id, pagina) {
            Some(p) => boton.position = p,
            None => {
                restaurar(deck, boton, &origen);
                return Err("La pagina esta llena.".to_string());
            }
        }
    }

    let ocupada = deck.surfaces[surface_id].pages[pagina]
        .buttons
        .iter()
        .any(|b| b.position == boton.position);

    if ocupada {
        // El desplazado va a la celda que deja libre el que llega, si venia de esta
        // misma pagina. Si no, al primer hueco; y si no hay, la operacion no cabe.
        let destino = match &origen {
            Some((sid, pi, pos)) if sid == surface_id && *pi == pagina => Some(*pos),
            _ => primera_libre(deck, surface_id, pagina),
        };
        let Some(destino) = destino else {
            restaurar(deck, boton, &origen);
            return Err("La pagina esta llena.".to_string());
        };

        let s = deck.surfaces.get_mut(surface_id).unwrap();
        if let Some(desplazado) = s.pages[pagina]
            .buttons
            .iter_mut()
            .find(|b| b.position == boton.position)
        {
            desplazado.position = destino;
        }
    }

    deck.surfaces.get_mut(surface_id).unwrap().pages[pagina]
        .buttons
        .push(boton);
    Ok(())
}

/// Elimina un boton y lo devuelve. Si era una carpeta, su superficie se conserva.
pub fn delete_button(deck: &mut Deck, button_id: &str) -> Result<DeckButton, String> {
    let (sid, pi, bi) =
        localizar(deck, button_id).ok_or_else(|| format!("No existe el boton {button_id}"))?;
    let boton = deck
        .surfaces
        .get_mut(&sid)
        .and_then(|s| s.pages.get_mut(pi))
        .map(|p| p.buttons.remove(bi))
        .ok_or_else(|| "No se pudo eliminar el boton".to_string())?;
    Ok(boton)
}

/// Duplica un boton en la primera celda libre de su misma pagina.
pub fn duplicate_button(deck: &mut Deck, button_id: &str) -> Result<String, String> {
    let (sid, pi, bi) =
        localizar(deck, button_id).ok_or_else(|| format!("No existe el boton {button_id}"))?;
    let mut copia = deck.surfaces[&sid].pages[pi].buttons[bi].clone();
    copia.id = nuevo_id(deck, "b");
    copia.position = primera_libre(deck, &sid, pi)
        .ok_or_else(|| "No queda sitio en esta pagina.".to_string())?;
    let id = copia.id.clone();

    deck.surfaces.get_mut(&sid).unwrap().pages[pi]
        .buttons
        .push(copia);
    Ok(id)
}

/// Mueve un boton a una celda, posiblemente de otra superficie o pagina.
pub fn move_button(
    deck: &mut Deck,
    button_id: &str,
    a_superficie: &str,
    a_pagina: usize,
    a_posicion: u32,
) -> Result<(), String> {
    let (sid, pi, bi) =
        localizar(deck, button_id).ok_or_else(|| format!("No existe el boton {button_id}"))?;
    let mut boton = deck.surfaces[&sid].pages[pi].buttons[bi].clone();
    boton.position = a_posicion;

    // Mover una carpeta dentro de su propia descendencia cerraria el lazo.
    if let Action::Folder { surface } = &boton.action {
        if integrity::would_create_cycle(deck, a_superficie, surface) {
            return Err("Ese movimiento crearia una navegacion circular.".to_string());
        }
    }

    upsert_button(deck, a_superficie, a_pagina, boton)
}

/// Crea una carpeta: la superficie nueva y la tecla que lleva a ella.
pub fn create_folder(
    deck: &mut Deck,
    surface_id: &str,
    pagina: usize,
    posicion: u32,
    nombre: &str,
) -> Result<String, String> {
    let nombre = if nombre.trim().is_empty() {
        "Carpeta"
    } else {
        nombre.trim()
    };

    let sid_nueva = nuevo_id(deck, "s");
    deck.surfaces
        .insert(sid_nueva.clone(), Surface::new(nombre));

    let boton = DeckButton {
        id: nuevo_id(deck, "b"),
        position: posicion,
        label: nombre.to_string(),
        icon: Icon {
            source: crate::model::IconSource::Builtin {
                name: "folder".into(),
            },
            ..Icon::default()
        },
        action: Action::Folder {
            surface: sid_nueva.clone(),
        },
    };
    let bid = boton.id.clone();

    if let Err(e) = upsert_button(deck, surface_id, pagina, boton) {
        // Si la tecla no cupo, no dejar la superficie suelta.
        deck.surfaces.remove(&sid_nueva);
        return Err(e);
    }
    Ok(bid)
}

/// Anade una pagina al final de una superficie.
pub fn add_page(deck: &mut Deck, surface_id: &str) -> Result<usize, String> {
    let s = deck
        .surfaces
        .get_mut(surface_id)
        .ok_or_else(|| format!("No existe la superficie {surface_id}"))?;
    s.pages.push(Page::default());
    Ok(s.pages.len() - 1)
}

/// Elimina una pagina vacia. Se niega a borrar una con teclas: que el usuario las
/// mueva o las borre primero, de forma explicita.
pub fn remove_page(deck: &mut Deck, surface_id: &str, pagina: usize) -> Result<(), String> {
    let s = deck
        .surfaces
        .get_mut(surface_id)
        .ok_or_else(|| format!("No existe la superficie {surface_id}"))?;
    if s.pages.len() <= 1 {
        return Err("Una superficie necesita al menos una pagina.".to_string());
    }
    let page = s
        .pages
        .get(pagina)
        .ok_or_else(|| "Esa pagina no existe.".to_string())?;
    if !page.buttons.is_empty() {
        return Err("La pagina todavia tiene teclas.".to_string());
    }
    s.pages.remove(pagina);
    Ok(())
}

/// Renombra una superficie (el nombre que sale en las migas de pan).
pub fn rename_surface(deck: &mut Deck, surface_id: &str, nombre: &str) -> Result<(), String> {
    let s = deck
        .surfaces
        .get_mut(surface_id)
        .ok_or_else(|| format!("No existe la superficie {surface_id}"))?;
    if nombre.trim().is_empty() {
        return Err("El nombre no puede quedar vacio.".to_string());
    }
    s.name = nombre.trim().to_string();
    Ok(())
}

/// Borra superficies que nadie referencia. Accion explicita del usuario.
pub fn purge_orphans(deck: &mut Deck) -> Vec<String> {
    let huerfanas = integrity::check(deck).orphans;
    for id in &huerfanas {
        deck.surfaces.remove(id);
    }
    huerfanas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::IconSource;
    use crate::store::default_deck;

    fn boton(id: &str, pos: u32) -> DeckButton {
        DeckButton {
            id: id.to_string(),
            position: pos,
            label: id.to_string(),
            icon: Icon::default(),
            action: Action::App {
                target: "notepad.exe".into(),
                args: String::new(),
                workdir: String::new(),
                focus_if_running: false,
            },
        }
    }

    fn contar(deck: &Deck, sid: &str, pagina: usize) -> usize {
        deck.surfaces[sid].pages[pagina].buttons.len()
    }

    // ------------------------------------------- la celda 0 de un perfil

    /// Un perfil se muestra como nivel superior, sin tecla de volver. Si el
    /// backend le reservara la celda 0 como a una carpeta, esa celda quedaria
    /// visualmente vacia e imposible de usar.
    #[test]
    fn la_celda_cero_de_un_perfil_se_puede_usar() {
        let mut deck = default_deck();
        deck.surfaces
            .insert("s-excel".into(), crate::model::Surface::new("Excel"));
        deck.profiles = vec![crate::perfiles::Profile {
            id: "p1".into(),
            surface: "s-excel".into(),
            exes: vec!["excel.exe".into()],
            enabled: true,
        }];

        upsert_button(&mut deck, "s-excel", 0, boton("b-nuevo", 0)).unwrap();

        let puesto = &deck.surfaces["s-excel"].pages[0].buttons[0];
        assert_eq!(puesto.position, 0, "la celda 0 del perfil quedo reservada");
    }

    /// Y lo contrario: una carpeta corriente sigue reservandola.
    #[test]
    fn la_celda_cero_de_una_carpeta_sigue_reservada() {
        let mut deck = default_deck();
        let raiz = deck.root.clone();
        create_folder(&mut deck, &raiz, 0, 1, "Carpeta").unwrap();
        let sid = deck
            .surfaces
            .iter()
            .find(|(_, s)| s.name == "Carpeta")
            .map(|(k, _)| k.clone())
            .expect("la carpeta deberia existir");

        upsert_button(&mut deck, &sid, 0, boton("b-nuevo", 0)).unwrap();

        let puesto = &deck.surfaces[&sid].pages[0].buttons[0];
        assert_ne!(puesto.position, 0, "piso la tecla de volver");
    }

    #[test]
    fn una_tecla_sin_id_recibe_uno_propio_y_unico() {
        let mut deck = default_deck();
        let mut a = boton("", 6);
        a.label = "Primera".into();
        let mut b = boton("", 7);
        b.label = "Segunda".into();

        upsert_button(&mut deck, "s-root", 0, a).unwrap();
        upsert_button(&mut deck, "s-root", 0, b).unwrap();

        let ids: Vec<&str> = deck.surfaces["s-root"].pages[0]
            .buttons
            .iter()
            .filter(|x| x.label == "Primera" || x.label == "Segunda")
            .map(|x| x.id.as_str())
            .collect();
        assert_eq!(ids.len(), 2, "las dos teclas deben existir");
        assert!(!ids[0].is_empty() && !ids[1].is_empty());
        assert_ne!(ids[0], ids[1], "no pueden compartir id");
    }

    #[test]
    fn crea_un_boton_en_la_celda_pedida() {
        let mut deck = default_deck();
        upsert_button(&mut deck, "s-root", 0, boton("b-nuevo", 7)).unwrap();
        let b = deck.surfaces["s-root"].pages[0]
            .buttons
            .iter()
            .find(|b| b.id == "b-nuevo")
            .unwrap();
        assert_eq!(b.position, 7);
    }

    #[test]
    fn dos_botones_en_la_misma_celda_intercambian_posicion() {
        let mut deck = default_deck();
        upsert_button(&mut deck, "s-root", 0, boton("b-a", 6)).unwrap();
        upsert_button(&mut deck, "s-root", 0, boton("b-b", 7)).unwrap();
        // Mover b-b encima de b-a: deben quedar intercambiados, no perderse.
        move_button(&mut deck, "b-b", "s-root", 0, 6).unwrap();

        let pos = |id: &str| {
            deck.surfaces["s-root"].pages[0]
                .buttons
                .iter()
                .find(|b| b.id == id)
                .unwrap()
                .position
        };
        assert_eq!(pos("b-b"), 6);
        assert_eq!(pos("b-a"), 7);
    }

    #[test]
    fn la_celda_cero_de_una_subcarpeta_queda_para_la_tecla_volver() {
        let mut deck = default_deck();
        upsert_button(&mut deck, "s-ejemplos", 0, boton("b-x", 0)).unwrap();
        let b = deck.surfaces["s-ejemplos"].pages[0]
            .buttons
            .iter()
            .find(|b| b.id == "b-x")
            .unwrap();
        assert_ne!(b.position, 0, "la celda 0 es de la tecla de volver");
    }

    #[test]
    fn borrar_una_tecla_de_carpeta_no_se_lleva_su_contenido() {
        let mut deck = default_deck();
        let teclas_dentro = contar(&deck, "s-ejemplos", 0);

        delete_button(&mut deck, "b-ejemplos").unwrap();

        assert!(
            deck.surfaces.contains_key("s-ejemplos"),
            "la superficie debe conservarse: un borrado accidental no puede \
             llevarse las teclas de dentro"
        );
        assert_eq!(contar(&deck, "s-ejemplos", 0), teclas_dentro);
        // Pero queda reportada como huerfana, para poder limpiarla a conciencia.
        assert_eq!(integrity::check(&deck).orphans, vec!["s-ejemplos"]);
    }

    #[test]
    fn purgar_huerfanas_es_lo_unico_que_borra_superficies() {
        let mut deck = default_deck();
        delete_button(&mut deck, "b-ejemplos").unwrap();
        let borradas = purge_orphans(&mut deck);
        assert_eq!(borradas, vec!["s-ejemplos"]);
        assert!(!deck.surfaces.contains_key("s-ejemplos"));
    }

    #[test]
    fn duplicar_da_un_id_nuevo_y_no_pisa_al_original() {
        let mut deck = default_deck();
        let copia = duplicate_button(&mut deck, "b-notepad").unwrap();
        assert_ne!(copia, "b-notepad");

        let pagina = &deck.surfaces["s-root"].pages[0];
        let orig = pagina.buttons.iter().find(|b| b.id == "b-notepad").unwrap();
        let dup = pagina.buttons.iter().find(|b| b.id == copia).unwrap();
        assert_ne!(orig.position, dup.position);
        assert_eq!(orig.label, dup.label);
    }

    #[test]
    fn crear_una_carpeta_deja_superficie_y_tecla_coherentes() {
        let mut deck = default_deck();
        let bid = create_folder(&mut deck, "s-root", 0, 9, "Proyectos").unwrap();

        let b = deck.surfaces["s-root"].pages[0]
            .buttons
            .iter()
            .find(|x| x.id == bid)
            .unwrap();
        let Action::Folder { surface } = &b.action else {
            panic!("deberia ser una carpeta");
        };
        assert_eq!(deck.surfaces[surface].name, "Proyectos");
        assert!(matches!(b.icon.source, IconSource::Builtin { .. }));
        assert!(integrity::check(&deck).is_clean());
    }

    #[test]
    fn renombrar_la_tecla_renombra_tambien_su_carpeta() {
        let mut deck = default_deck();
        let bid = create_folder(&mut deck, "s-root", 0, 9, "Carpeta").unwrap();

        // Editar la tecla cambiandole la etiqueta, como hace el editor.
        let (sid, pi, bi) = localizar(&deck, &bid).unwrap();
        let mut editada = deck.surfaces[&sid].pages[pi].buttons[bi].clone();
        editada.label = "Lista Master".to_string();
        let Action::Folder { surface } = editada.action.clone() else {
            panic!("deberia ser una carpeta");
        };
        upsert_button(&mut deck, "s-root", 0, editada).unwrap();

        // Sin esto, las migas de pan y el desplegable seguirian diciendo "Carpeta".
        assert_eq!(deck.surfaces[&surface].name, "Lista Master");
    }

    #[test]
    fn una_etiqueta_vacia_no_borra_el_nombre_de_la_carpeta() {
        let mut deck = default_deck();
        let bid = create_folder(&mut deck, "s-root", 0, 9, "Proyectos").unwrap();
        let (sid, pi, bi) = localizar(&deck, &bid).unwrap();
        let mut editada = deck.surfaces[&sid].pages[pi].buttons[bi].clone();
        editada.label = "   ".to_string();
        let Action::Folder { surface } = editada.action.clone() else {
            panic!()
        };
        upsert_button(&mut deck, "s-root", 0, editada).unwrap();
        assert_eq!(deck.surfaces[&surface].name, "Proyectos");
    }

    #[test]
    fn mover_una_carpeta_dentro_de_si_misma_se_rechaza() {
        let mut deck = default_deck();
        // b-ejemplos lleva a s-ejemplos; meterlo dentro de s-ejemplos seria un lazo.
        let r = move_button(&mut deck, "b-ejemplos", "s-ejemplos", 0, 5);
        assert!(r.is_err(), "deberia rechazarse el ciclo");
        assert!(
            integrity::check(&deck).is_clean(),
            "el deck no debe quedar tocado"
        );
    }

    #[test]
    fn una_carpeta_que_no_cupo_no_deja_superficie_suelta() {
        let mut deck = default_deck();
        // Llenar la raiz entera.
        for i in 0..deck.settings.grid.cells() {
            let _ = upsert_button(&mut deck, "s-root", 0, boton(&format!("b-relleno{i}"), i));
        }
        let antes = deck.surfaces.len();
        let r = create_folder(&mut deck, "s-root", 0, 99, "No cabe");
        assert!(r.is_err());
        assert_eq!(
            deck.surfaces.len(),
            antes,
            "no debe quedar una superficie huerfana"
        );
    }

    #[test]
    fn mover_un_boton_a_otra_superficie_lo_saca_de_la_anterior() {
        let mut deck = default_deck();
        move_button(&mut deck, "b-notepad", "s-ejemplos", 0, 5).unwrap();

        assert!(deck.surfaces["s-root"].pages[0]
            .buttons
            .iter()
            .all(|b| b.id != "b-notepad"));
        assert!(deck.surfaces["s-ejemplos"].pages[0]
            .buttons
            .iter()
            .any(|b| b.id == "b-notepad"));
    }

    #[test]
    fn no_se_borra_una_pagina_con_teclas() {
        let mut deck = default_deck();
        add_page(&mut deck, "s-root").unwrap();
        assert!(remove_page(&mut deck, "s-root", 0).is_err());
        // La pagina vacia que se acaba de anadir si se puede quitar.
        assert!(remove_page(&mut deck, "s-root", 1).is_ok());
    }

    #[test]
    fn no_se_borra_la_ultima_pagina() {
        let mut deck = default_deck();
        assert!(remove_page(&mut deck, "s-ejemplos", 0).is_err());
    }

    #[test]
    fn un_movimiento_que_no_cabe_devuelve_el_boton_a_su_sitio() {
        let mut deck = default_deck();
        // Llenar s-ejemplos del todo (celda 0 es de la tecla de volver).
        for i in 1..deck.settings.grid.cells() {
            let _ = upsert_button(&mut deck, "s-ejemplos", 0, boton(&format!("b-lleno{i}"), i));
        }

        let antes = deck.surfaces["s-root"].pages[0]
            .buttons
            .iter()
            .find(|b| b.id == "b-notepad")
            .unwrap()
            .position;

        let r = move_button(&mut deck, "b-notepad", "s-ejemplos", 0, 3);
        assert!(r.is_err(), "no cabia, deberia fallar");

        // Lo importante: el boton no se evapora por el camino.
        let despues = deck.surfaces["s-root"].pages[0]
            .buttons
            .iter()
            .find(|b| b.id == "b-notepad")
            .expect("el boton debe seguir en su superficie original");
        assert_eq!(despues.position, antes, "y en su celda original");
    }

    #[test]
    fn los_ids_generados_no_chocan_con_los_existentes() {
        let deck = default_deck();
        let ids: Vec<String> = (0..50).map(|_| nuevo_id(&deck, "b")).collect();
        let unicos: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unicos.len(), ids.len(), "hubo ids repetidos");
        assert!(!ids.iter().any(|i| i == "b-notepad"));
    }
}
