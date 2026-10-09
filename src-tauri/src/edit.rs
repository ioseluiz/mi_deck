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
        states: Vec::new(),
        wheel: None,
        live: None,
        extra: Default::default(),
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

// ------------------------------------------------------------------- perfiles

/// Crea un perfil con su superficie propia, vacia.
///
/// El ejecutable se guarda siempre como nombre suelto: si alguien pega una ruta
/// completa, se queda con el ultimo tramo. Es la misma clave que produce el gancho
/// de primer plano, y si no coincidieran el perfil no se activaria nunca.
pub fn create_profile(deck: &mut Deck, exe: &str, nombre: &str) -> Result<String, String> {
    let exe = crate::focus::nombre_de_ejecutable(exe);
    if exe.is_empty() {
        return Err("Hay que indicar la aplicacion.".to_string());
    }
    // Un segundo perfil para la misma aplicacion no se activaria nunca, porque
    // gana el primero. Vale mas decirlo que dejarlo ahi sin funcionar.
    if let Some(ya) = crate::perfiles::perfil_para(&deck.profiles, &exe) {
        let como = deck
            .surfaces
            .get(&ya.surface)
            .map(|s| s.name.as_str())
            .unwrap_or("otro");
        return Err(format!("Ya hay un perfil para {exe}: «{como}»."));
    }

    let nombre = if nombre.trim().is_empty() {
        exe.clone()
    } else {
        nombre.trim().to_string()
    };

    let sid = nuevo_id(deck, "s");
    deck.surfaces.insert(sid.clone(), Surface::new(&nombre));
    deck.profiles.push(crate::perfiles::Profile {
        id: nuevo_id(deck, "p"),
        surface: sid.clone(),
        exes: vec![exe],
        enabled: true,
    });
    Ok(sid)
}

/// Quita el perfil **sin borrar su superficie**.
///
/// Sigue la regla de todo el modulo: nada se borra en cascada. Sus teclas quedan
/// guardadas y la superficie pasa a estar huerfana, igual que al borrar una tecla
/// de carpeta; recuperarlas es volver a crear el perfil o apuntarles una carpeta.
pub fn delete_profile(deck: &mut Deck, id: &str) -> Result<(), String> {
    let antes = deck.profiles.len();
    deck.profiles.retain(|p| p.id != id);
    if deck.profiles.len() == antes {
        return Err(format!("No existe el perfil {id}"));
    }
    Ok(())
}

/// Habilita o deshabilita un perfil sin perder nada.
/// Cambia el nombre de un perfil, que es el de su superficie.
///
/// El nombre sale del titulo de la ventana al crearlo, y un titulo no siempre da
/// algo presentable. Sin esto habia que borrar el perfil y rehacerlo, perdiendo
/// las teclas por una cuestion de texto.
pub fn rename_profile(deck: &mut Deck, id: &str, nombre: &str) -> Result<(), String> {
    let nombre = nombre.trim();
    if nombre.is_empty() {
        return Err("El perfil necesita un nombre.".to_string());
    }
    let perfil = buscar(deck, id)?;
    let superficie = perfil.surface.clone();
    deck.surfaces
        .get_mut(&superficie)
        .ok_or_else(|| format!("El perfil apunta a un panel que no existe: {superficie}"))?
        .name = nombre.to_string();
    Ok(())
}

/// Anade otro ejecutable al mismo perfil.
///
/// El mismo programa llega con nombres distintos segun como este instalado, y hay
/// familias --un visor y su editor, una suite-- donde las mismas teclas valen para
/// varios. La lista ya existia en el modelo; lo que faltaba era poder tocarla.
pub fn add_profile_exe(deck: &mut Deck, id: &str, exe: &str) -> Result<(), String> {
    let exe = crate::focus::nombre_de_ejecutable(exe);
    if exe.is_empty() {
        return Err("Hay que indicar la aplicacion.".to_string());
    }

    // Gana el primer perfil que empareje, asi que repetir un ejecutable en otro
    // perfil crea una regla que no se cumpliria nunca.
    if let Some(ya) = crate::perfiles::perfil_para(&deck.profiles, &exe) {
        if ya.id != id {
            let como = deck
                .surfaces
                .get(&ya.surface)
                .map(|s| s.name.as_str())
                .unwrap_or("otro");
            return Err(format!("Ya hay un perfil para {exe}: «{como}»."));
        }
        return Err(format!("Este perfil ya cubre {exe}."));
    }

    buscar(deck, id)?;
    let perfil = deck.profiles.iter_mut().find(|p| p.id == id).unwrap();
    if perfil.exes.iter().any(|e| e.eq_ignore_ascii_case(&exe)) {
        return Err(format!("Este perfil ya cubre {exe}."));
    }
    perfil.exes.push(exe);
    Ok(())
}

/// Quita un ejecutable de un perfil.
///
/// Nunca el ultimo: un perfil sin ejecutables no se activaria jamas y se quedaria
/// como una fila muerta en Ajustes. Para eso esta quitar el perfil entero, que
/// ademas avisa de cuantas teclas se llevaria.
pub fn remove_profile_exe(deck: &mut Deck, id: &str, exe: &str) -> Result<(), String> {
    let perfil = deck
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("No existe el perfil {id}"))?;

    if perfil.exes.len() <= 1 {
        return Err(
            "Es el unico ejecutable del perfil: sin el no se activaria nunca.              Quita el perfil entero si ya no lo quieres."
                .to_string(),
        );
    }
    let antes = perfil.exes.len();
    perfil.exes.retain(|e| !e.eq_ignore_ascii_case(exe));
    if perfil.exes.len() == antes {
        return Err(format!("El perfil no cubre {exe}."));
    }
    Ok(())
}

/// Copia las teclas de un panel a otro sin tocar las que ya hubiera.
///
/// Montar un segundo perfil parecido costaba teclearlo todo otra vez. Copiar solo
/// a celdas libres es deliberado: nunca puede destruir trabajo, asi que no hace
/// falta una confirmacion que nadie lee. Devuelve cuantas teclas se quedaron
/// fuera por falta de sitio, para poder decirlo en vez de perderlas en silencio.
///
/// Las carpetas se copian enteras. Si se compartiera el panel de destino, editar
/// una carpeta en un perfil cambiaria la del otro, que es lo contrario de lo que
/// espera quien acaba de pedir una copia.
pub fn copy_keys(deck: &mut Deck, desde: &str, hasta: &str) -> Result<usize, String> {
    if desde == hasta {
        return Err("El origen y el destino son el mismo panel.".to_string());
    }
    if !deck.surfaces.contains_key(desde) {
        return Err(format!("No existe el panel {desde}"));
    }
    if !deck.surfaces.contains_key(hasta) {
        return Err(format!("No existe el panel {hasta}"));
    }

    // Un mapa de panel viejo a panel nuevo: ademas de no copiar dos veces la misma
    // carpeta, corta en seco un ciclo de carpetas que se apunten entre si.
    let mut copiados: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    copiados.insert(desde.to_string(), hasta.to_string());

    let paginas = deck.surfaces[desde].pages.len();
    let mut sin_sitio = 0;

    for i in 0..paginas {
        let originales: Vec<DeckButton> = deck.surfaces[desde].pages[i].buttons.clone();
        for boton in originales {
            // La celda de volver de la carpeta de origen no se copia: el destino
            // la pinta sola si le corresponde.
            if boton.position == 0 && celda_cero_reservada(deck, desde) {
                continue;
            }
            while deck.surfaces[hasta].pages.len() <= i {
                deck.surfaces
                    .get_mut(hasta)
                    .unwrap()
                    .pages
                    .push(Page::default());
            }
            let Some(destino) = hueco_para(deck, hasta, i, boton.position) else {
                sin_sitio += 1;
                continue;
            };

            let mut copia = boton.clone();
            copia.id = nuevo_id(deck, "b");
            copia.position = destino;
            if let Action::Folder { surface } = &copia.action {
                let suyo = clonar_panel(deck, surface, &mut copiados);
                copia.action = Action::Folder { surface: suyo };
            }
            deck.surfaces.get_mut(hasta).unwrap().pages[i]
                .buttons
                .push(copia);
        }
    }
    Ok(sin_sitio)
}

/// Copia una tecla a otro sitio del deck, sin tocar la original.
///
/// Hace falta para sacar una tecla de una carpeta: dentro de ella no se puede
/// arrastrar al nivel de arriba, porque ese nivel no esta en pantalla. Con copiar
/// y pegar se cruza cualquier frontera, y de paso se puede repetir una tecla en
/// varios paneles sin volver a configurarla.
///
/// Si la tecla es una carpeta, el panel entero se copia igual que en `copy_keys`:
/// compartirlo haria que editar la copia cambiara el original, que es lo contrario
/// de lo que espera quien pega una copia.
pub fn copy_button(
    deck: &mut Deck,
    button_id: &str,
    a_superficie: &str,
    a_pagina: usize,
    a_posicion: u32,
) -> Result<String, String> {
    let (sid, pi, bi) =
        localizar(deck, button_id).ok_or_else(|| format!("No existe el boton {button_id}"))?;
    let mut copia = deck.surfaces[&sid].pages[pi].buttons[bi].clone();

    if !deck.surfaces.contains_key(a_superficie) {
        return Err(format!("No existe el panel {a_superficie}"));
    }
    asegurar_pagina(deck, a_superficie, a_pagina)?;

    let Some(destino) = hueco_para(deck, a_superficie, a_pagina, a_posicion) else {
        return Err("La pagina esta llena.".to_string());
    };

    if let Action::Folder { surface } = &copia.action {
        let mut ya = std::collections::HashMap::new();
        let suyo = clonar_panel(deck, surface, &mut ya);
        copia.action = Action::Folder { surface: suyo };
    }
    copia.id = nuevo_id(deck, "b");
    copia.position = destino;

    let id = copia.id.clone();
    deck.surfaces.get_mut(a_superficie).unwrap().pages[a_pagina]
        .buttons
        .push(copia);
    Ok(id)
}

/// Celda libre del destino: la misma de origen si esta libre, si no la primera.
fn hueco_para(deck: &Deck, surface_id: &str, pagina: usize, preferida: u32) -> Option<u32> {
    let reservada = celda_cero_reservada(deck, surface_id);
    let ocupadas: Vec<u32> = deck
        .surfaces
        .get(surface_id)
        .and_then(|s| s.pages.get(pagina))
        .map(|p| p.buttons.iter().map(|b| b.position).collect())
        .unwrap_or_default();

    if preferida < celdas(deck) && !(reservada && preferida == 0) && !ocupadas.contains(&preferida)
    {
        return Some(preferida);
    }
    primera_libre(deck, surface_id, pagina)
}

/// Copia un panel entero y devuelve el identificador del nuevo.
///
/// `ya` recuerda lo copiado en esta misma operacion: sin eso, dos teclas que
/// apunten a la misma carpeta darian dos copias, y un ciclo no terminaria nunca.
fn clonar_panel(
    deck: &mut Deck,
    origen: &str,
    ya: &mut std::collections::HashMap<String, String>,
) -> String {
    if let Some(hecho) = ya.get(origen) {
        return hecho.clone();
    }
    let Some(viejo) = deck.surfaces.get(origen).cloned() else {
        // Una referencia rota se copia tal cual: no es cosa de esta funcion
        // arreglarla, y `integrity` ya la reporta.
        return origen.to_string();
    };

    let nuevo = nuevo_id(deck, "s");
    ya.insert(origen.to_string(), nuevo.clone());
    deck.surfaces
        .insert(nuevo.clone(), Surface::new(&viejo.name));

    for (i, pagina) in viejo.pages.iter().enumerate() {
        while deck.surfaces[&nuevo].pages.len() <= i {
            deck.surfaces
                .get_mut(&nuevo)
                .unwrap()
                .pages
                .push(Page::default());
        }
        for boton in &pagina.buttons {
            let mut copia = boton.clone();
            copia.id = nuevo_id(deck, "b");
            if let Action::Folder { surface } = &copia.action {
                let suyo = clonar_panel(deck, surface, ya);
                copia.action = Action::Folder { surface: suyo };
            }
            deck.surfaces.get_mut(&nuevo).unwrap().pages[i]
                .buttons
                .push(copia);
        }
    }
    nuevo
}

/// Perfil por identificador, o un error que lo nombre.
fn buscar<'a>(deck: &'a Deck, id: &str) -> Result<&'a crate::perfiles::Profile, String> {
    deck.profiles
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("No existe el perfil {id}"))
}

pub fn set_profile_enabled(deck: &mut Deck, id: &str, enabled: bool) -> Result<(), String> {
    let p = deck
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("No existe el perfil {id}"))?;
    p.enabled = enabled;
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

    /// Un deck con un perfil recien creado y su identificador.
    fn con_perfil() -> (Deck, String) {
        let mut deck = default_deck();
        create_profile(&mut deck, "excel.exe", "Excel").unwrap();
        let id = deck.profiles[0].id.clone();
        (deck, id)
    }

    #[test]
    fn copiar_una_tecla_a_otro_panel_deja_la_original_donde_estaba() {
        // El caso que lo motivo: sacar una tecla de una carpeta, donde arrastrar
        // no llega porque el nivel de arriba no esta en pantalla.
        let mut deck = default_deck();
        deck.surfaces.insert("s-sub".into(), Surface::new("Dentro"));
        deck.surfaces.get_mut("s-sub").unwrap().pages = vec![Page {
            buttons: vec![boton("atrapada", 1)],
        }];

        let root = deck.root.clone();
        let nuevo = copy_button(&mut deck, "atrapada", &root, 0, 4).unwrap();

        // La original sigue dentro.
        assert_eq!(deck.surfaces["s-sub"].pages[0].buttons.len(), 1);
        // Y hay una copia fuera, con identificador propio.
        let raiz = &deck.surfaces[&root].pages[0].buttons;
        let copia = raiz.iter().find(|b| b.id == nuevo).expect("deberia estar");
        assert_eq!(copia.position, 4);
        assert_ne!(copia.id, "atrapada");
    }

    #[test]
    fn copiar_una_carpeta_le_da_su_propio_panel() {
        // Compartirlo haria que editar la copia cambiara el original.
        let mut deck = default_deck();
        deck.surfaces
            .insert("s-sub".into(), Surface::new("Carpeta"));
        deck.surfaces.get_mut("s-sub").unwrap().pages = vec![Page {
            buttons: vec![boton("dentro", 1)],
        }];
        let mut carpeta = boton("bc", 4);
        carpeta.action = Action::Folder {
            surface: "s-sub".into(),
        };
        let root = deck.root.clone();
        deck.surfaces.get_mut(&root).unwrap().pages[0]
            .buttons
            .push(carpeta);

        let nuevo = copy_button(&mut deck, "bc", &root, 0, 8).unwrap();

        let raiz = &deck.surfaces[&root].pages[0].buttons;
        let copia = raiz.iter().find(|b| b.id == nuevo).unwrap();
        let Action::Folder { surface } = &copia.action else {
            panic!("deberia seguir siendo una carpeta");
        };
        assert_ne!(surface, "s-sub", "la carpeta quedo compartida");
        assert_eq!(deck.surfaces[surface].pages[0].buttons.len(), 1);
    }

    #[test]
    fn pegar_en_una_celda_ocupada_busca_hueco_en_vez_de_pisar() {
        let mut deck = default_deck();
        let root = deck.root.clone();
        let ocupada = deck.surfaces[&root].pages[0].buttons[0].position;
        let id = deck.surfaces[&root].pages[0].buttons[0].id.clone();
        let antes = deck.surfaces[&root].pages[0].buttons.len();

        let nuevo = copy_button(&mut deck, &id, &root, 0, ocupada).unwrap();

        let raiz = &deck.surfaces[&root].pages[0].buttons;
        assert_eq!(raiz.len(), antes + 1, "no se perdio ninguna");
        let copia = raiz.iter().find(|b| b.id == nuevo).unwrap();
        assert_ne!(copia.position, ocupada, "la copia piso a la que estaba");
    }

    #[test]
    fn copiar_teclas_llena_un_panel_vacio_sin_mover_nada() {
        let mut deck = default_deck();
        let a = create_profile(&mut deck, "excel.exe", "Excel").unwrap();
        let b = create_profile(&mut deck, "winword.exe", "Word").unwrap();
        deck.surfaces.get_mut(&a).unwrap().pages = vec![Page {
            buttons: vec![boton("b1", 3), boton("b2", 7)],
        }];

        let fuera = copy_keys(&mut deck, &a, &b).unwrap();

        assert_eq!(fuera, 0);
        let copiadas = &deck.surfaces[&b].pages[0].buttons;
        assert_eq!(copiadas.len(), 2);
        // Mismas celdas, porque estaban libres.
        assert_eq!(copiadas[0].position, 3);
        assert_eq!(copiadas[1].position, 7);
        // Identificadores nuevos: dos teclas no pueden compartir el mismo.
        assert_ne!(copiadas[0].id, "b1");
    }

    #[test]
    fn copiar_teclas_no_pisa_las_que_ya_hubiera() {
        // Nunca destruye trabajo: por eso no hace falta confirmacion.
        let mut deck = default_deck();
        let a = create_profile(&mut deck, "excel.exe", "Excel").unwrap();
        let b = create_profile(&mut deck, "winword.exe", "Word").unwrap();
        deck.surfaces.get_mut(&a).unwrap().pages = vec![Page {
            buttons: vec![boton("b1", 3)],
        }];
        deck.surfaces.get_mut(&b).unwrap().pages = vec![Page {
            buttons: vec![boton("suya", 3)],
        }];

        copy_keys(&mut deck, &a, &b).unwrap();

        let destino = &deck.surfaces[&b].pages[0].buttons;
        assert_eq!(destino.len(), 2);
        assert_eq!(destino[0].id, "suya", "la que ya estaba sigue en su celda");
        assert_ne!(destino[1].position, 3, "la copia se fue a otra celda");
    }

    #[test]
    fn las_teclas_que_no_caben_se_cuentan_en_vez_de_perderse() {
        let mut deck = default_deck();
        deck.settings.grid = crate::model::Grid { cols: 2, rows: 1 };
        let a = create_profile(&mut deck, "excel.exe", "Excel").unwrap();
        let b = create_profile(&mut deck, "winword.exe", "Word").unwrap();
        deck.surfaces.get_mut(&a).unwrap().pages = vec![Page {
            buttons: vec![boton("b1", 0), boton("b2", 1)],
        }];
        deck.surfaces.get_mut(&b).unwrap().pages = vec![Page {
            buttons: vec![boton("suya", 0)],
        }];

        let fuera = copy_keys(&mut deck, &a, &b).unwrap();
        assert_eq!(fuera, 1, "una no cabia y hay que poder decirlo");
    }

    #[test]
    fn una_carpeta_copiada_es_suya_y_no_la_del_otro_perfil() {
        // Si se compartiera, editar la carpeta en un perfil cambiaria la del
        // otro, que es lo contrario de lo que espera quien pide una copia.
        let mut deck = default_deck();
        let a = create_profile(&mut deck, "excel.exe", "Excel").unwrap();
        let b = create_profile(&mut deck, "winword.exe", "Word").unwrap();
        deck.surfaces
            .insert("s-sub".into(), Surface::new("Pegado especial"));
        deck.surfaces.get_mut("s-sub").unwrap().pages = vec![Page {
            buttons: vec![boton("dentro", 1)],
        }];
        let mut carpeta = boton("bc", 2);
        carpeta.action = Action::Folder {
            surface: "s-sub".into(),
        };
        deck.surfaces.get_mut(&a).unwrap().pages = vec![Page {
            buttons: vec![carpeta],
        }];

        copy_keys(&mut deck, &a, &b).unwrap();

        let Action::Folder { surface } = &deck.surfaces[&b].pages[0].buttons[0].action else {
            panic!("deberia seguir siendo una carpeta");
        };
        assert_ne!(surface, "s-sub", "la carpeta quedo compartida");
        assert_eq!(deck.surfaces[surface].pages[0].buttons.len(), 1);
    }

    #[test]
    fn un_panel_no_se_copia_sobre_si_mismo() {
        let mut deck = default_deck();
        let a = create_profile(&mut deck, "excel.exe", "Excel").unwrap();
        assert!(copy_keys(&mut deck, &a, &a).is_err());
    }

    #[test]
    fn renombrar_un_perfil_cambia_el_nombre_de_su_panel() {
        let (mut deck, id) = con_perfil();
        rename_profile(&mut deck, &id, "  Hojas de calculo  ").unwrap();

        let sid = &deck.profiles[0].surface;
        assert_eq!(deck.surfaces[sid].name, "Hojas de calculo");
    }

    #[test]
    fn un_perfil_no_se_queda_sin_nombre() {
        let (mut deck, id) = con_perfil();
        assert!(rename_profile(&mut deck, &id, "   ").is_err());
        assert_eq!(deck.surfaces[&deck.profiles[0].surface].name, "Excel");
    }

    #[test]
    fn un_perfil_puede_cubrir_varios_ejecutables() {
        let (mut deck, id) = con_perfil();
        add_profile_exe(&mut deck, &id, "C:/Office/WINWORD.EXE").unwrap();

        // Se guarda el nombre suelto, que es con lo que se compara despues.
        assert_eq!(deck.profiles[0].exes, vec!["excel.exe", "winword.exe"]);
    }

    #[test]
    fn no_se_puede_robar_el_ejecutable_de_otro_perfil() {
        // Gana el primero que empareje, asi que la segunda regla no se cumpliria
        // nunca: es mejor decirlo que dejarla ahi sin funcionar.
        let (mut deck, _) = con_perfil();
        create_profile(&mut deck, "winword.exe", "Word").unwrap();
        let word = deck.profiles[1].id.clone();

        let Err(motivo) = add_profile_exe(&mut deck, &word, "EXCEL.EXE") else {
            panic!("deberia haberse quejado");
        };
        assert!(
            motivo.contains("Excel"),
            "deberia nombrar el perfil: {motivo}"
        );
    }

    #[test]
    fn repetir_el_mismo_ejecutable_no_lo_duplica() {
        let (mut deck, id) = con_perfil();
        assert!(add_profile_exe(&mut deck, &id, "Excel.exe").is_err());
        assert_eq!(deck.profiles[0].exes.len(), 1);
    }

    #[test]
    fn quitar_el_ultimo_ejecutable_dejaria_el_perfil_muerto() {
        // Sin ejecutables no se activaria jamas y se quedaria como una fila que
        // no hace nada. Para eso esta quitar el perfil entero.
        let (mut deck, id) = con_perfil();
        let Err(motivo) = remove_profile_exe(&mut deck, &id, "excel.exe") else {
            panic!("deberia haberse quejado");
        };
        assert!(motivo.contains("unico"), "{motivo}");
        assert_eq!(deck.profiles[0].exes.len(), 1);
    }

    #[test]
    fn quitar_un_ejecutable_cuando_hay_otro_si_vale() {
        let (mut deck, id) = con_perfil();
        add_profile_exe(&mut deck, &id, "winword.exe").unwrap();
        remove_profile_exe(&mut deck, &id, "EXCEL.EXE").unwrap();

        assert_eq!(deck.profiles[0].exes, vec!["winword.exe"]);
    }

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
            states: Vec::new(),
            wheel: None,
            live: None,
            extra: Default::default(),
        }
    }

    fn contar(deck: &Deck, sid: &str, pagina: usize) -> usize {
        deck.surfaces[sid].pages[pagina].buttons.len()
    }

    // ----------------------------------------------------------- perfiles

    #[test]
    fn crear_un_perfil_deja_su_superficie_vacia_y_lista() {
        let mut deck = default_deck();
        let sid = create_profile(&mut deck, "EXCEL.EXE", "Excel").unwrap();

        assert_eq!(deck.profiles.len(), 1);
        // Siempre el nombre suelto y en minusculas: es la clave que produce el
        // gancho de primer plano.
        assert_eq!(deck.profiles[0].exes, vec!["excel.exe"]);
        assert_eq!(deck.surfaces[&sid].name, "Excel");
        assert!(deck.profiles[0].enabled);
    }

    #[test]
    fn una_ruta_completa_se_reduce_al_nombre_del_ejecutable() {
        let mut deck = default_deck();
        create_profile(
            &mut deck,
            r"C:\Program Files\Microsoft Office\root\Office16\EXCEL.EXE",
            "",
        )
        .unwrap();
        assert_eq!(deck.profiles[0].exes, vec!["excel.exe"]);
        // Sin nombre, se usa el del ejecutable.
        let sid = &deck.profiles[0].surface;
        assert_eq!(deck.surfaces[sid].name, "excel.exe");
    }

    /// Un segundo perfil para la misma aplicacion no se activaria nunca, porque
    /// gana el primero. Vale mas decirlo que dejarlo ahi sin funcionar.
    #[test]
    fn no_se_pueden_crear_dos_perfiles_para_la_misma_app() {
        let mut deck = default_deck();
        create_profile(&mut deck, "excel.exe", "Excel").unwrap();

        let Err(motivo) = create_profile(&mut deck, "Excel.exe", "Otro") else {
            panic!("deberia rechazarse el duplicado");
        };
        assert!(motivo.contains("Excel"), "mensaje poco util: {motivo}");
        assert_eq!(deck.profiles.len(), 1);
    }

    #[test]
    fn un_perfil_sin_aplicacion_se_rechaza() {
        let mut deck = default_deck();
        assert!(create_profile(&mut deck, "   ", "Algo").is_err());
        assert!(deck.profiles.is_empty());
    }

    /// Nada se borra en cascada, la regla de todo el modulo: quitar el perfil no
    /// puede llevarse las teclas que el usuario configuro en el.
    #[test]
    fn borrar_un_perfil_conserva_sus_teclas() {
        let mut deck = default_deck();
        let sid = create_profile(&mut deck, "excel.exe", "Excel").unwrap();
        upsert_button(&mut deck, &sid, 0, boton("b-excel", 0)).unwrap();

        let id = deck.profiles[0].id.clone();
        delete_profile(&mut deck, &id).unwrap();

        assert!(deck.profiles.is_empty());
        assert!(deck.surfaces.contains_key(&sid), "se llevo la superficie");
        assert_eq!(contar(&deck, &sid, 0), 1, "se llevo las teclas");
    }

    #[test]
    fn borrar_un_perfil_que_no_existe_da_error() {
        let mut deck = default_deck();
        assert!(delete_profile(&mut deck, "p-inventado").is_err());
    }

    #[test]
    fn deshabilitar_un_perfil_no_pierde_nada() {
        let mut deck = default_deck();
        let sid = create_profile(&mut deck, "excel.exe", "Excel").unwrap();
        upsert_button(&mut deck, &sid, 0, boton("b-excel", 0)).unwrap();
        let id = deck.profiles[0].id.clone();

        set_profile_enabled(&mut deck, &id, false).unwrap();
        assert!(!deck.profiles[0].enabled);
        assert_eq!(contar(&deck, &sid, 0), 1);

        set_profile_enabled(&mut deck, &id, true).unwrap();
        assert!(deck.profiles[0].enabled);
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
