//! Envio de teclas: el primitivo del que cuelgan casi todas las acciones de
//! Windows.
//!
//! Partido igual que `launcher`: `parsear()` es pura y esta cubierta por tests,
//! y `enviar()` solo llama a SendInput. Casi todo lo que un Stream Deck hace con
//! Windows se alcanza enviando una combinacion, asi que merece la pena que esta
//! pieza sea solida.
//!
//! Dos invariantes:
//!   - Los modificadores se sueltan SIEMPRE, incluso si el envio falla a medias.
//!     Dejar Win o Ctrl pulsados deja el equipo inservible hasta que el usuario
//!     los pulse a mano.
//!   - Las teclas de navegacion llevan la marca de extendida. Sin ella, Windows
//!     puede interpretar una flecha como la tecla equivalente del teclado
//!     numerico.

// Codigos de tecla virtual. Se usan como u16 y no como el tipo del crate
// `windows` para que el parser se pueda probar sin tocar la API del sistema.
pub const VK_SHIFT: u16 = 0x10;
pub const VK_CONTROL: u16 = 0x11;
pub const VK_ALT: u16 = 0x12;
pub const VK_LWIN: u16 = 0x5B;

const VK_BACK: u16 = 0x08;
const VK_TAB: u16 = 0x09;
const VK_RETURN: u16 = 0x0D;
const VK_ESCAPE: u16 = 0x1B;
const VK_SPACE: u16 = 0x20;
const VK_PRIOR: u16 = 0x21;
const VK_NEXT: u16 = 0x22;
const VK_END: u16 = 0x23;
const VK_HOME: u16 = 0x24;
const VK_LEFT: u16 = 0x25;
const VK_UP: u16 = 0x26;
const VK_RIGHT: u16 = 0x27;
const VK_DOWN: u16 = 0x28;
const VK_SNAPSHOT: u16 = 0x2C;
const VK_INSERT: u16 = 0x2D;
const VK_DELETE: u16 = 0x2E;
const VK_F1: u16 = 0x70;

pub const VK_VOLUME_MUTE: u16 = 0xAD;
pub const VK_VOLUME_DOWN: u16 = 0xAE;
pub const VK_VOLUME_UP: u16 = 0xAF;
pub const VK_MEDIA_NEXT: u16 = 0xB0;
pub const VK_MEDIA_PREV: u16 = 0xB1;
pub const VK_MEDIA_PLAY_PAUSE: u16 = 0xB3;

const VK_OEM_COMMA: u16 = 0xBC;
const VK_OEM_PERIOD: u16 = 0xBE;

/// Una combinacion lista para enviar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combinacion {
    /// En el orden en que se pulsan. Se sueltan en el inverso.
    pub modificadores: Vec<u16>,
    pub tecla: u16,
}

/// Teclas que exigen la marca de extendida para que Windows no las confunda con
/// las del teclado numerico.
fn es_extendida(vk: u16) -> bool {
    matches!(
        vk,
        VK_LEFT
            | VK_UP
            | VK_RIGHT
            | VK_DOWN
            | VK_HOME
            | VK_END
            | VK_PRIOR
            | VK_NEXT
            | VK_INSERT
            | VK_DELETE
            | VK_SNAPSHOT
    )
}

/// Traduce el nombre de una tecla a su codigo virtual.
///
/// Acepta alias en espanol y en ingles porque el usuario escribe la combinacion a
/// mano y no tiene por que adivinar cual espera el programa.
fn vk_de_nombre(nombre: &str) -> Option<u16> {
    let n = nombre.trim().to_ascii_lowercase();

    // F1 a F24.
    if let Some(resto) = n.strip_prefix('f') {
        if let Ok(numero) = resto.parse::<u16>() {
            if (1..=24).contains(&numero) {
                return Some(VK_F1 + numero - 1);
            }
        }
    }

    // Una letra o un digito suelto.
    if n.chars().count() == 1 {
        let c = n.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            return Some(c.to_ascii_uppercase() as u16);
        }
        if c.is_ascii_digit() {
            return Some(c as u16);
        }
        if c == '.' {
            return Some(VK_OEM_PERIOD);
        }
        if c == ',' {
            return Some(VK_OEM_COMMA);
        }
    }

    Some(match n.as_str() {
        "esc" | "escape" => VK_ESCAPE,
        "tab" | "tabulador" => VK_TAB,
        "intro" | "enter" | "return" => VK_RETURN,
        "space" | "spacebar" | "espacio" => VK_SPACE,
        "back" | "backspace" | "retroceso" => VK_BACK,
        "supr" | "del" | "delete" => VK_DELETE,
        "ins" | "insert" | "insertar" => VK_INSERT,
        "inicio" | "home" => VK_HOME,
        "fin" | "end" => VK_END,
        "pageup" | "pgup" | "pagarriba" => VK_PRIOR,
        "pagedown" | "pgdn" | "pagabajo" => VK_NEXT,
        "left" | "izquierda" => VK_LEFT,
        "right" | "derecha" => VK_RIGHT,
        "up" | "arriba" => VK_UP,
        "down" | "abajo" => VK_DOWN,
        "prtscn" | "printscreen" | "imprpant" => VK_SNAPSHOT,
        "period" | "punto" => VK_OEM_PERIOD,
        "comma" | "coma" => VK_OEM_COMMA,
        "volumeup" | "subirvolumen" => VK_VOLUME_UP,
        "volumedown" | "bajarvolumen" => VK_VOLUME_DOWN,
        "volumemute" | "silenciar" => VK_VOLUME_MUTE,
        "medianext" | "siguiente" => VK_MEDIA_NEXT,
        "mediaprev" | "anterior" => VK_MEDIA_PREV,
        "mediaplaypause" | "reproducir" => VK_MEDIA_PLAY_PAUSE,
        _ => return None,
    })
}

fn vk_de_modificador(nombre: &str) -> Option<u16> {
    match nombre.trim().to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(VK_CONTROL),
        "shift" | "mayus" | "mayusculas" => Some(VK_SHIFT),
        "alt" => Some(VK_ALT),
        "win" | "super" | "meta" | "windows" => Some(VK_LWIN),
        _ => None,
    }
}

/// Interpreta una combinacion escrita por el usuario: "Ctrl+Shift+S", "win + d".
pub fn parsear(combinacion: &str) -> Result<Combinacion, String> {
    let partes: Vec<&str> = combinacion
        .split('+')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();

    if partes.is_empty() {
        return Err("La combinacion esta vacia.".to_string());
    }

    let mut modificadores = Vec::new();
    let mut tecla = None;

    for (i, parte) in partes.iter().enumerate() {
        let ultima = i == partes.len() - 1;

        // La ultima parte es la tecla; las anteriores, modificadores. Asi
        // "Ctrl+Alt" avisa de que falta la tecla en vez de enviar algo raro.
        if ultima {
            tecla = vk_de_nombre(parte);
            if tecla.is_none() {
                return Err(format!(
                    "No se reconoce la tecla \"{parte}\". Usa una letra, un digito, \
                     F1 a F24, o un nombre como Esc, Tab, Supr, Inicio o Izquierda."
                ));
            }
        } else {
            match vk_de_modificador(parte) {
                Some(vk) => {
                    if !modificadores.contains(&vk) {
                        modificadores.push(vk);
                    }
                }
                None => {
                    return Err(format!(
                        "\"{parte}\" no es un modificador. Los validos son Ctrl, \
                         Shift, Alt y Win."
                    ))
                }
            }
        }
    }

    let tecla = tecla
        .ok_or_else(|| "La combinacion solo tiene modificadores: falta la tecla.".to_string())?;

    // "Ctrl+Ctrl" dejaria modificadores pero ninguna tecla real distinta.
    if modificadores.contains(&tecla) {
        return Err("La tecla final no puede ser un modificador.".to_string());
    }

    Ok(Combinacion {
        modificadores,
        tecla,
    })
}

// ------------------------------------------------------------------- envio

/// Envia la combinacion al sistema.
#[cfg(not(windows))]
pub fn enviar(_combinacion: &str) -> Result<(), String> {
    Err("Enviar teclas solo esta implementado en Windows.".into())
}

#[cfg(windows)]
pub fn enviar(combinacion: &str) -> Result<(), String> {
    let c = parsear(combinacion)?;
    let mut eventos = Vec::with_capacity((c.modificadores.len() + 1) * 2);

    for m in &c.modificadores {
        eventos.push(evento(*m, false));
    }
    eventos.push(evento(c.tecla, false));
    eventos.push(evento(c.tecla, true));
    // Al reves: el ultimo pulsado es el primero en soltarse.
    for m in c.modificadores.iter().rev() {
        eventos.push(evento(*m, true));
    }

    mandar(&eventos)
}

/// Teclea un texto literal.
///
/// Con KEYEVENTF_UNICODE no hace falta traducir a codigos de tecla, asi que
/// funciona con tildes, enes y emoji sin depender de la distribucion del teclado.
#[cfg(not(windows))]
pub fn escribir(_texto: &str) -> Result<(), String> {
    Err("Teclear texto solo esta implementado en Windows.".into())
}

#[cfg(windows)]
pub fn escribir(texto: &str) -> Result<(), String> {
    if texto.is_empty() {
        return Err("No hay texto que escribir.".to_string());
    }
    let mut eventos = Vec::new();
    // encode_utf16 parte los emoji en pares subrogados, que es justo lo que
    // SendInput espera: un evento por unidad de 16 bits.
    for unidad in texto.encode_utf16() {
        eventos.push(evento_unicode(unidad, false));
        eventos.push(evento_unicode(unidad, true));
    }
    mandar(&eventos)
}

/// Las teclas de medios y de volumen son globales: no hace falta devolver el foco
/// a nadie para que funcionen.
#[cfg(windows)]
pub fn enviar_vk(vk: u16) -> Result<(), String> {
    mandar(&[evento(vk, false), evento(vk, true)])
}

#[cfg(not(windows))]
pub fn enviar_vk(_vk: u16) -> Result<(), String> {
    Err("Enviar teclas solo esta implementado en Windows.".into())
}

#[cfg(windows)]
fn evento(vk: u16, soltar: bool) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    let mut flags = KEYBD_EVENT_FLAGS(0);
    if soltar {
        flags |= KEYEVENTF_KEYUP;
    }
    if es_extendida(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(windows)]
fn evento_unicode(unidad: u16, soltar: bool) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY,
    };

    let mut flags = KEYEVENTF_UNICODE;
    if soltar {
        flags |= KEYEVENTF_KEYUP;
    }

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unidad,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(windows)]
fn mandar(eventos: &[windows::Win32::UI::Input::KeyboardAndMouse::INPUT]) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};

    let enviados = unsafe { SendInput(eventos, std::mem::size_of::<INPUT>() as i32) } as usize;

    if enviados == eventos.len() {
        return Ok(());
    }

    // Si se corto a medias puede haber modificadores pulsados. Soltarlos todos es
    // inofensivo si no lo estaban, y evita dejar el teclado bloqueado.
    soltar_modificadores();
    Err(format!(
        "El sistema acepto {enviados} de {} eventos de teclado. Puede haber otro \
         programa bloqueando la entrada sintetica.",
        eventos.len()
    ))
}

/// Suelta los cuatro modificadores, por si un envio quedo a medias.
#[cfg(windows)]
pub fn soltar_modificadores() {
    let eventos: Vec<_> = [VK_CONTROL, VK_SHIFT, VK_ALT, VK_LWIN]
        .iter()
        .map(|vk| evento(*vk, true))
        .collect();
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};
    unsafe { SendInput(&eventos, std::mem::size_of::<INPUT>() as i32) };
}

#[cfg(not(windows))]
pub fn soltar_modificadores() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpreta_una_combinacion_corriente() {
        let c = parsear("Ctrl+Shift+S").unwrap();
        assert_eq!(c.modificadores, vec![VK_CONTROL, VK_SHIFT]);
        assert_eq!(c.tecla, b'S' as u16);
    }

    #[test]
    fn no_distingue_mayusculas_ni_espacios() {
        // El usuario escribe esto a mano: tiene que perdonar el formato.
        let a = parsear("ctrl+shift+s").unwrap();
        let b = parsear("  CTRL + Shift +  S  ").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn acepta_alias_en_espanol_y_en_ingles() {
        assert_eq!(parsear("Win+D").unwrap().modificadores, vec![VK_LWIN]);
        assert_eq!(parsear("Super+D").unwrap().modificadores, vec![VK_LWIN]);
        assert_eq!(
            parsear("Ctrl+Supr").unwrap().tecla,
            parsear("Ctrl+Delete").unwrap().tecla
        );
        assert_eq!(
            parsear("Alt+Intro").unwrap().tecla,
            parsear("Alt+Enter").unwrap().tecla
        );
        assert_eq!(
            parsear("Ctrl+Izquierda").unwrap().tecla,
            parsear("Ctrl+Left").unwrap().tecla
        );
    }

    #[test]
    fn cubre_todo_el_rango_de_teclas_de_funcion() {
        assert_eq!(parsear("F1").unwrap().tecla, 0x70);
        assert_eq!(parsear("F12").unwrap().tecla, 0x7B);
        assert_eq!(parsear("F24").unwrap().tecla, 0x87);
        assert!(parsear("F25").is_err());
        assert!(parsear("F0").is_err());
    }

    #[test]
    fn una_tecla_sola_sin_modificadores_vale() {
        let c = parsear("F5").unwrap();
        assert!(c.modificadores.is_empty());
        assert_eq!(c.tecla, 0x74);
    }

    #[test]
    fn el_atajo_de_captura_de_windows_se_interpreta() {
        let c = parsear("Win+Shift+S").unwrap();
        assert_eq!(c.modificadores, vec![VK_LWIN, VK_SHIFT]);
        assert_eq!(c.tecla, b'S' as u16);
    }

    #[test]
    fn una_combinacion_vacia_es_un_error_claro() {
        assert!(parsear("").is_err());
        assert!(parsear("   ").is_err());
        assert!(parsear("+++").is_err());
    }

    #[test]
    fn solo_modificadores_avisa_de_que_falta_la_tecla() {
        // Enviar "Ctrl+Alt" sin tecla no haria nada util; mejor rechazarlo al
        // guardar que dejar una tecla muerta en el panel.
        let e = parsear("Ctrl+Alt").unwrap_err();
        assert!(e.to_lowercase().contains("tecla"), "mensaje poco util: {e}");
    }

    #[test]
    fn una_tecla_inexistente_se_nombra_en_el_error() {
        let e = parsear("Ctrl+Inventada").unwrap_err();
        assert!(
            e.contains("Inventada"),
            "el error deberia citar la tecla: {e}"
        );
    }

    #[test]
    fn un_modificador_mal_escrito_se_nombra_en_el_error() {
        let e = parsear("Contro+S").unwrap_err();
        assert!(
            e.contains("Contro"),
            "el error deberia citar el modificador: {e}"
        );
    }

    #[test]
    fn no_se_repite_un_modificador() {
        let c = parsear("Ctrl+Ctrl+S").unwrap();
        assert_eq!(c.modificadores, vec![VK_CONTROL]);
    }

    #[test]
    fn la_tecla_final_no_puede_ser_un_modificador() {
        assert!(parsear("Ctrl+Shift").is_err());
        assert!(parsear("Alt+Win").is_err());
    }

    #[test]
    fn las_teclas_de_navegacion_van_marcadas_como_extendidas() {
        // Sin la marca, Windows puede tomar una flecha por la del teclado numerico.
        for nombre in [
            "Left", "Right", "Up", "Down", "Home", "Fin", "PageUp", "Supr",
        ] {
            let c = parsear(nombre).unwrap();
            assert!(es_extendida(c.tecla), "{nombre} deberia ser extendida");
        }
        // Una letra no lo es.
        assert!(!es_extendida(parsear("S").unwrap().tecla));
    }

    #[test]
    fn el_punto_y_la_coma_se_reconocen() {
        // Win+. abre el selector de emoji.
        assert_eq!(parsear("Win+.").unwrap().tecla, VK_OEM_PERIOD);
        assert_eq!(parsear("Win+punto").unwrap().tecla, VK_OEM_PERIOD);
        assert_eq!(parsear("Ctrl+,").unwrap().tecla, VK_OEM_COMMA);
    }

    #[test]
    fn los_digitos_se_reconocen() {
        assert_eq!(parsear("Ctrl+1").unwrap().tecla, b'1' as u16);
        assert_eq!(parsear("Alt+0").unwrap().tecla, b'0' as u16);
    }

    #[test]
    fn el_orden_de_los_modificadores_se_respeta() {
        // Se sueltan en orden inverso al de pulsacion, asi que el orden importa.
        assert_eq!(
            parsear("Ctrl+Win+Left").unwrap().modificadores,
            vec![VK_CONTROL, VK_LWIN]
        );
        assert_eq!(
            parsear("Win+Ctrl+Left").unwrap().modificadores,
            vec![VK_LWIN, VK_CONTROL]
        );
    }

    #[test]
    fn escribir_texto_vacio_es_un_error() {
        assert!(escribir("").is_err());
    }
}
