//! Validacion de la posicion de la ventana contra los monitores conectados.
//!
//! Dos formas de perder el widget fuera de la pantalla, ambas vistas en uso real:
//!
//!   1. Al minimizar, Windows reporta la posicion como (-32000, -32000). Si esa
//!      lectura se guarda, al reabrir la ventana aparece a 32 000 px del
//!      escritorio: el proceso corre, pero no hay forma de verla ni de alcanzarla.
//!   2. La posicion se guardo en un monitor que despues se desconecto.
//!
//! Por eso la posicion se valida al guardarla y al restaurarla. Si no queda
//! alcanzable, se descarta y la ventana se centra.

use crate::model::WindowPos;

/// Coordenada por debajo de la cual se asume que Windows esta reportando una
/// ventana minimizada o fuera de todo escritorio razonable.
const CENTINELA_MINIMIZADA: i32 = -30_000;

/// Cuanta ventana debe quedar dentro de un monitor para considerarla usable.
/// Con menos que esto no se puede ni agarrar la barra de titulo para moverla.
const MIN_ANCHO_VISIBLE: i32 = 140;
const MIN_ALTO_VISIBLE: i32 = 40;

/// Rectangulo de un monitor, en coordenadas del escritorio virtual.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Si una ventana de ese tamano colocada ahi quedaria al alcance del raton.
pub fn es_alcanzable(pos: WindowPos, tam: (u32, u32), monitores: &[Rect]) -> bool {
    if pos.x <= CENTINELA_MINIMIZADA || pos.y <= CENTINELA_MINIMIZADA {
        return false;
    }
    let (ancho, alto) = (tam.0 as i32, tam.1 as i32);

    monitores.iter().any(|m| {
        let solape_x = (pos.x + ancho).min(m.x + m.w as i32) - pos.x.max(m.x);
        let solape_y = (pos.y + alto).min(m.y + m.h as i32) - pos.y.max(m.y);
        solape_x >= MIN_ANCHO_VISIBLE.min(ancho) && solape_y >= MIN_ALTO_VISIBLE.min(alto)
    })
}

/// La posicion guardada, solo si sigue siendo utilizable.
///
/// Devolver None significa "deja que la ventana se centre sola", que es el
/// comportamiento correcto cuando la posicion guardada ya no tiene sentido.
pub fn posicion_segura(
    guardada: Option<WindowPos>,
    tam: (u32, u32),
    monitores: &[Rect],
) -> Option<WindowPos> {
    let pos = guardada?;
    if monitores.is_empty() {
        // Sin informacion de monitores, al menos descartar el centinela.
        return (pos.x > CENTINELA_MINIMIZADA && pos.y > CENTINELA_MINIMIZADA).then_some(pos);
    }
    es_alcanzable(pos, tam, monitores).then_some(pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El par de monitores del equipo donde aparecio el fallo.
    fn dos_monitores() -> Vec<Rect> {
        vec![
            Rect {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080,
            },
            Rect {
                x: 1920,
                y: 0,
                w: 1920,
                h: 1200,
            },
        ]
    }

    const TAM: (u32, u32) = (596, 509);

    fn pos(x: i32, y: i32) -> WindowPos {
        WindowPos { x, y }
    }

    #[test]
    fn la_posicion_de_una_ventana_minimizada_se_rechaza() {
        // Es el caso que dejo el widget invisible: Windows reporta esto al
        // minimizar, y guardarlo equivale a perder la ventana.
        assert!(!es_alcanzable(pos(-32_000, -32_000), TAM, &dos_monitores()));
        assert_eq!(
            posicion_segura(Some(pos(-32_000, -32_000)), TAM, &dos_monitores()),
            None
        );
    }

    #[test]
    fn una_posicion_normal_se_conserva() {
        let p = pos(729, 143);
        assert!(es_alcanzable(p, TAM, &dos_monitores()));
        assert_eq!(posicion_segura(Some(p), TAM, &dos_monitores()), Some(p));
    }

    #[test]
    fn el_monitor_secundario_tambien_vale() {
        let p = pos(2400, 300);
        assert!(es_alcanzable(p, TAM, &dos_monitores()));
    }

    #[test]
    fn si_se_desconecta_el_monitor_la_posicion_deja_de_valer() {
        let p = pos(2400, 300);
        let solo_principal = vec![Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        }];
        assert!(!es_alcanzable(p, TAM, &solo_principal));
        assert_eq!(posicion_segura(Some(p), TAM, &solo_principal), None);
    }

    #[test]
    fn una_ventana_asomando_apenas_por_el_borde_se_considera_perdida() {
        // 40 px visibles no bastan para agarrar la barra de titulo.
        assert!(!es_alcanzable(pos(1880, 500), TAM, &[dos_monitores()[0]]));
        // Con medio panel dentro, si.
        assert!(es_alcanzable(pos(1600, 500), TAM, &[dos_monitores()[0]]));
    }

    #[test]
    fn una_ventana_por_encima_del_borde_superior_se_rechaza() {
        // Arrastrada hacia arriba hasta dejar la barra de titulo fuera.
        assert!(!es_alcanzable(pos(400, -500), TAM, &dos_monitores()));
    }

    #[test]
    fn sin_posicion_guardada_no_hay_nada_que_restaurar() {
        assert_eq!(posicion_segura(None, TAM, &dos_monitores()), None);
    }

    #[test]
    fn sin_monitores_conocidos_solo_se_filtra_el_centinela() {
        assert_eq!(
            posicion_segura(Some(pos(100, 100)), TAM, &[]),
            Some(pos(100, 100))
        );
        assert_eq!(posicion_segura(Some(pos(-32_000, -32_000)), TAM, &[]), None);
    }
}
