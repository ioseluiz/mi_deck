//! Modelo de datos del deck.
//!
//! La estructura es un registro plano de superficies con referencias, no un arbol
//! anidado: una superficie es un nivel navegable (la raiz, o el interior de una
//! carpeta) y un boton de tipo `folder` apunta a otra superficie por id. Mover una
//! carpeta de sitio es cambiar una linea, no recortar y pegar un bloque anidado.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Acepta tanto una lista como un elemento suelto.
///
/// PowerShell es la herramienta mas a mano para editar un JSON en Windows, y su
/// `ConvertTo-Json` serializa una lista de un solo elemento como ese elemento,
/// sin corchetes. Un deck.json que pase por ahi deja de encajar con el formato y
/// el widget lo da por corrupto: lo respalda y arranca de cero. Tolerarlo aqui
/// cuesta diez lineas y evita perder el trabajo por usar la herramienta obvia.
fn uno_o_varios<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum UnoOVarios<T> {
        Varios(Vec<T>),
        Uno(Box<T>),
    }

    Ok(match UnoOVarios::<T>::deserialize(d)? {
        UnoOVarios::Varios(v) => v,
        UnoOVarios::Uno(u) => vec![*u],
    })
}

/// Version del esquema de `deck.json`. Se incrementa al romper compatibilidad.
pub const SCHEMA_VERSION: u32 = 1;

fn default_version() -> u32 {
    SCHEMA_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deck {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub settings: Settings,
    /// Id de la superficie raiz.
    pub root: String,
    pub surfaces: HashMap<String, Surface>,
    /// Perfiles por aplicacion, en orden. Gana el primero que empareje.
    ///
    /// Viven aqui y no en `Settings` por dos motivos concretos: `update_settings`
    /// reemplaza los ajustes enteros con lo que mande el frontend, asi que un
    /// descuido en el editor borraria todos los perfiles; y un perfil referencia
    /// superficies, que es contenido del deck y no una preferencia de ventana.
    #[serde(default)]
    pub profiles: Vec<crate::perfiles::Profile>,
    /// Lo que esta version no conoce, conservado tal cual.
    ///
    /// Sin esto, abrir con una version anterior un archivo escrito por una mas
    /// nueva perderia silenciosamente lo que la anterior no entiende: serde lo
    /// ignora al leer y no lo escribe al guardar. Es la misma red que ya tiene
    /// `Action::Unknown`, pero para el deck entero.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------- ajustes

/// Donde vive el panel respecto a las demas ventanas.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowLevel {
    /// Ventana corriente: entra y sale del frente con el orden z habitual.
    Normal,
    /// Por encima de todo, siempre. Comodo de alcanzar, pero tapa el trabajo.
    #[default]
    Top,
    /// Al nivel del escritorio: por debajo de cualquier otra ventana, como los
    /// antiguos gadgets. Nunca estorba; se ve con Win+D o con el atajo global.
    Desktop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub grid: Grid,
    pub key_size: u32,
    pub window_level: WindowLevel,
    /// Con el candado puesto, arrastrar la barra de titulo no mueve la ventana.
    pub lock_position: bool,
    /// Combinacion global que trae el panel al frente. None lo desactiva.
    pub hotkey: Option<String>,
    pub opacity: f32,
    pub start_with_windows: bool,
    pub start_minimized: bool,
    /// Donde se guardan las capturas. Vacio = Imagenes\MiDeck del usuario.
    pub screenshot_dir: String,
    /// Ademas de guardarla, dejarla en el portapapeles lista para pegar.
    pub screenshot_to_clipboard: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowPos>,
    /// Ajustes que esta version no conoce. Ver `Deck::extra`.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            grid: Grid::default(),
            key_size: 96,
            window_level: WindowLevel::default(),
            lock_position: false,
            hotkey: Some("Ctrl+Alt+Space".to_string()),
            opacity: 1.0,
            start_with_windows: false,
            start_minimized: false,
            screenshot_dir: String::new(),
            // Por defecto si: casi siempre la captura es para pegarla en un
            // correo o en un ticket, no para dejarla en una carpeta.
            screenshot_to_clipboard: true,
            window: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// Rejilla 5 x 3 = 15 teclas, el formato del Stream Deck MK.2.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct Grid {
    pub cols: u32,
    pub rows: u32,
}

impl Default for Grid {
    fn default() -> Self {
        Self { cols: 5, rows: 3 }
    }
}

impl Grid {
    /// Cantidad de celdas por pagina.
    pub fn cells(&self) -> u32 {
        self.cols * self.rows
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowPos {
    pub x: i32,
    pub y: i32,
}

// ------------------------------------------------------- superficies y paginas

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Surface {
    pub name: String,
    #[serde(default, deserialize_with = "uno_o_varios")]
    pub pages: Vec<Page>,
}

impl Surface {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            pages: vec![Page::default()],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Page {
    #[serde(default, deserialize_with = "uno_o_varios")]
    pub buttons: Vec<DeckButton>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckButton {
    pub id: String,
    /// Indice de celda dentro de la pagina (0..cells-1). Permite huecos.
    pub position: u32,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub icon: Icon,
    #[serde(deserialize_with = "accion_tolerante")]
    pub action: Action,
}

// -------------------------------------------------------------------- iconos

/// Cara de la tecla: una fuente de imagen mas los ajustes de presentacion, que
/// son validos para cualquiera de las cuatro fuentes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Icon {
    #[serde(flatten)]
    pub source: IconSource,
    #[serde(default)]
    pub fit: Fit,
    #[serde(default)]
    pub label_style: LabelStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
}

impl Default for Icon {
    fn default() -> Self {
        Self {
            source: IconSource::Auto,
            fit: Fit::default(),
            label_style: LabelStyle::default(),
            background: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IconSource {
    /// Imagen importada a la biblioteca, nombrada por hash de contenido.
    Image { file: String },
    /// Extraido del destino de la accion (icono de shell del .exe/.lnk).
    Auto,
    /// Del set propio que viaja con la aplicacion.
    Builtin { name: String },
    Emoji {
        #[serde(rename = "char")]
        glyph: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Imagen centrada ocupando ~60% de la tecla, etiqueta debajo.
    #[default]
    Contain,
    /// Imagen a sangre cubriendo toda la cara de la tecla.
    Cover,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelStyle {
    #[default]
    Below,
    /// Sobre la imagen, con degradado al pie para que se lea sobre cualquier foto.
    Overlay,
    #[serde(rename = "none")]
    Hidden,
}

// ------------------------------------------------------------------ acciones

/// Enum etiquetado: el compilador obliga a cubrir todos los tipos en cada `match`,
/// asi que anadir uno nuevo no se puede olvidar a medias.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Lanza un .exe o .lnk.
    App {
        target: String,
        #[serde(default)]
        args: String,
        #[serde(default)]
        workdir: String,
        #[serde(default)]
        focus_if_running: bool,
    },
    /// Abre una direccion web.
    Url {
        target: String,
        #[serde(default = "default_browser")]
        browser: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<String>,
    },
    /// Abre varias direcciones de una vez, como pestanas de una misma ventana.
    ///
    /// Existe porque la peticion recurrente es "una tecla que me abra las seis
    /// paginas con las que trabajo". Chrome no permite abrir un grupo de
    /// pestanas guardado desde fuera -- no hay opcion de linea de comandos ni
    /// esquema de URL para eso -- pero si acepta varias direcciones de golpe, y
    /// eso cubre el caso real.
    Urls {
        #[serde(default, deserialize_with = "uno_o_varios")]
        targets: Vec<String>,
        #[serde(default = "default_browser")]
        browser: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<String>,
        /// Ventana nueva en vez de pestanas en la que ya hubiera abierta.
        #[serde(default)]
        new_window: bool,
    },
    /// Abre una carpeta o archivo en el Explorador de Windows.
    /// Ojo: distinto de `Folder`, que navega dentro del propio deck.
    Path { target: String },
    /// Ejecuta un script o comando.
    Script {
        #[serde(default)]
        shell: Shell,
        target: String,
        #[serde(default)]
        args: String,
        #[serde(default)]
        hidden: bool,
    },
    /// Navega a otra superficie del deck: la carpeta del Stream Deck.
    Folder { surface: String },

    /// Envia una combinacion de teclas: "Ctrl+Shift+S", "Win+D", "F5".
    ///
    /// Es el primitivo del que cuelgan casi todas las acciones de Windows: los
    /// atajos de sistema, los de cualquier aplicacion y el catalogo de `system`.
    Hotkey { keys: String },

    /// Teclea un texto literal en la aplicacion que estuviera delante.
    Text { text: String },

    /// Accion del catalogo de Windows: multimedia, volumen, bloquear, capturar.
    ///
    /// El catalogo vive en `sistema.rs` y dice como se lleva a cabo cada una;
    /// aqui solo se guarda cual es.
    System {
        command: crate::sistema::SystemCommand,
    },

    /// Varias acciones en orden, con pausas entre ellas.
    ///
    /// Es lo que hace falta para automatizar una funcion de una aplicacion:
    /// «pegar solo valores» en Excel son cuatro pulsaciones con una pausa en medio
    /// para que el menu llegue a abrirse.
    Macro {
        #[serde(default)]
        steps: Vec<MacroStep>,
    },

    /// Un tipo de accion que esta version no conoce.
    ///
    /// Es la red de seguridad para volver a una version anterior. Sin ella, un
    /// `deck.json` escrito por una version mas nueva no encaja con el formato, se
    /// da por corrupto, se respalda y el usuario ve que ha perdido sus teclas
    /// enteras por una sola que no se entendia.
    ///
    /// Se guarda el objeto original tal cual, bajo una clave aparte para no
    /// chocar con la etiqueta `type`, de modo que al volver a una version que si
    /// lo entienda la tecla reaparezca intacta.
    Unknown {
        #[serde(rename = "__original")]
        raw: serde_json::Map<String, serde_json::Value>,
    },
}

/// Lee una accion sin que una desconocida invalide el archivo entero.
///
/// Tres casos:
///   - se entiende: tal cual;
///   - es un envoltorio dejado por una version anterior y ahora **si** se
///     entiende: se recupera, que es lo que hace util guardar el original;
///   - no se entiende: se envuelve y se conserva byte a byte.
fn accion_tolerante<'de, D>(d: D) -> Result<Action, D::Error>
where
    D: Deserializer<'de>,
{
    let valor = serde_json::Value::deserialize(d)?;

    match serde_json::from_value::<Action>(valor.clone()) {
        Ok(Action::Unknown { raw }) => {
            let interior = serde_json::Value::Object(raw.clone());
            match serde_json::from_value::<Action>(interior) {
                // Ojo con el envoltorio anidado: si el interior tampoco se
                // entiende, se deja el de fuera y no se envuelve otra vez.
                Ok(Action::Unknown { .. }) | Err(_) => Ok(Action::Unknown { raw }),
                Ok(recuperada) => Ok(recuperada),
            }
        }
        Ok(conocida) => Ok(conocida),
        Err(_) => Ok(Action::Unknown {
            raw: match valor {
                serde_json::Value::Object(m) => m,
                otro => {
                    // Ni siquiera es un objeto. Se envuelve igual: perder la
                    // tecla es peor que arrastrar un valor raro.
                    let mut m = serde_json::Map::new();
                    m.insert("__valor".to_string(), otro);
                    m
                }
            },
        }),
    }
}

/// Un paso de una macro.
///
/// La pausa es **despues** del paso y no antes, que es como se piensa al
/// escribirla: «manda Ctrl+L, espera a que la barra tome el foco, escribe cmd».
/// Asi tampoco hace falta un tipo de paso «esperar»: la espera cuelga del paso
/// anterior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroStep {
    pub action: Box<Action>,
    #[serde(default)]
    pub delay_ms: u32,
}

fn default_browser() -> String {
    "default".to_string()
}

#[cfg(test)]
mod tests_tolerancia {
    use super::*;

    fn boton(accion: &str) -> String {
        format!(r#"{{"id":"b1","position":0,"label":"X","action":{accion}}}"#)
    }

    fn leer(accion: &str) -> DeckButton {
        serde_json::from_str(&boton(accion)).expect("el boton deberia leerse")
    }

    #[test]
    fn una_accion_conocida_se_lee_como_siempre() {
        let b = leer(r#"{"type":"path","target":"C:\\Datos"}"#);
        assert_eq!(b.action.kind(), "path");
    }

    /// El caso que motiva todo esto: una version anterior abriendo un archivo
    /// escrito por una mas nueva. Antes, esto tumbaba el deck entero.
    #[test]
    fn un_tipo_desconocido_no_invalida_el_boton() {
        let b = leer(r#"{"type":"capture_window","destino":"portapapeles"}"#);
        assert_eq!(b.action.kind(), "unknown");
        assert_eq!(b.action.tipo_original(), Some("capture_window"));
    }

    #[test]
    fn un_tipo_desconocido_no_invalida_el_deck_entero() {
        // Lo importante no es la tecla rara: es que las otras sobrevivan.
        let json = format!(
            r#"{{"version":1,"root":"r","surfaces":{{"r":{{"name":"Raiz","pages":[{{"buttons":[{},{}]}}]}}}}}}"#,
            boton(r#"{"type":"del_futuro","x":1}"#),
            boton(r#"{"type":"url","target":"https://a","browser":"default"}"#)
        );
        let deck: Deck = serde_json::from_str(&json).expect("el deck deberia leerse");
        let botones = &deck.surfaces["r"].pages[0].buttons;
        assert_eq!(botones.len(), 2);
        assert_eq!(botones[0].action.kind(), "unknown");
        assert_eq!(botones[1].action.kind(), "url");
    }

    /// Guardar no puede ser la forma de perder la tecla: lo que no se entiende
    /// tiene que volver al archivo tal y como entro.
    #[test]
    fn lo_desconocido_sobrevive_a_una_vuelta_completa() {
        let original = r#"{"type":"capture_window","destino":"portapapeles","n":7}"#;
        let b = leer(original);
        let guardado = serde_json::to_string(&b).unwrap();
        let releido: DeckButton = serde_json::from_str(&guardado).unwrap();

        let Action::Unknown { raw } = &releido.action else {
            panic!("deberia seguir sin entenderse");
        };
        let esperado: serde_json::Value = serde_json::from_str(original).unwrap();
        assert_eq!(serde_json::Value::Object(raw.clone()), esperado);
    }

    /// Y la otra mitad: al actualizar, la tecla tiene que volver a funcionar
    /// sola, sin que nadie la reescriba a mano.
    #[test]
    fn al_actualizar_una_tecla_envuelta_se_recupera() {
        // Lo que habria dejado en disco una version que no conocia `system`.
        let envuelto = r#"{"type":"unknown","__original":{"type":"system","command":"volume_up"}}"#;
        let b = leer(envuelto);
        assert_eq!(b.action.kind(), "system");
    }

    #[test]
    fn un_envoltorio_que_sigue_sin_entenderse_no_se_envuelve_dos_veces() {
        let envuelto = r#"{"type":"unknown","__original":{"type":"aun_mas_nuevo","x":1}}"#;
        let b = leer(envuelto);
        assert_eq!(b.action.tipo_original(), Some("aun_mas_nuevo"));
    }

    #[test]
    fn una_accion_que_ni_siquiera_es_un_objeto_no_tumba_nada() {
        let b = leer(r#""una cadena suelta""#);
        assert_eq!(b.action.kind(), "unknown");
    }

    // ------------------------------------- campos del deck que no se conocen

    fn deck_minimo(extra_deck: &str, extra_ajustes: &str) -> String {
        format!(
            r#"{{"version":1,"root":"r","surfaces":{{"r":{{"name":"Raiz","pages":[]}}}},
               "settings":{{"key_size":96{extra_ajustes}}}{extra_deck}}}"#
        )
    }

    /// La otra mitad de la red de seguridad. `Action::Unknown` protege una tecla
    /// suelta; esto protege lo que una version futura anada al deck o a los
    /// ajustes. Sin ello, abrir el archivo con una version anterior lo perderia
    /// en silencio al primer guardado.
    #[test]
    fn lo_que_el_deck_no_conoce_sobrevive_a_guardar() {
        let json = deck_minimo(
            r#","futuro_del_deck":{"algo":1},"otra_cosa":[1,2]"#,
            r#","futuro_de_ajustes":"x""#,
        );
        let deck: Deck = serde_json::from_str(&json).expect("deberia leerse");

        let guardado = serde_json::to_string(&deck).unwrap();
        let valor: serde_json::Value = serde_json::from_str(&guardado).unwrap();

        assert_eq!(valor["futuro_del_deck"]["algo"], 1);
        assert_eq!(valor["otra_cosa"], serde_json::json!([1, 2]));
        assert_eq!(valor["settings"]["futuro_de_ajustes"], "x");
        // Y lo conocido sigue en su sitio.
        assert_eq!(valor["root"], "r");
        assert_eq!(valor["settings"]["key_size"], 96);
    }

    #[test]
    fn un_deck_sin_campos_raros_no_gana_ninguno() {
        let deck: Deck = serde_json::from_str(&deck_minimo("", "")).unwrap();
        assert!(deck.extra.is_empty());
        assert!(deck.settings.extra.is_empty());
        assert!(deck.profiles.is_empty());
    }

    #[test]
    fn los_perfiles_se_leen_y_se_vuelven_a_escribir() {
        let json = deck_minimo(
            r#","profiles":[{"id":"p1","surface":"s-excel","exes":["excel.exe"]}]"#,
            "",
        );
        let deck: Deck = serde_json::from_str(&json).unwrap();
        assert_eq!(deck.profiles.len(), 1);
        // `enabled` no venia en el archivo y tiene que llegar en true.
        assert!(deck.profiles[0].enabled);

        let valor: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&deck).unwrap()).unwrap();
        assert_eq!(valor["profiles"][0]["surface"], "s-excel");
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shell {
    #[default]
    Powershell,
    Cmd,
}

impl Action {
    /// Nombre corto del tipo, para mensajes de error legibles.
    pub fn kind(&self) -> &'static str {
        match self {
            Action::App { .. } => "app",
            Action::Url { .. } => "url",
            Action::Urls { .. } => "urls",
            Action::Path { .. } => "path",
            Action::Script { .. } => "script",
            Action::Folder { .. } => "folder",
            Action::Hotkey { .. } => "hotkey",
            Action::Text { .. } => "text",
            Action::System { .. } => "system",
            Action::Macro { .. } => "macro",
            Action::Unknown { .. } => "unknown",
        }
    }

    /// Como se llamaba el tipo en el archivo, cuando no se entiende.
    ///
    /// Sirve para que el mensaje diga "esta tecla es de tipo capture_window" en
    /// vez de un generico "accion desconocida", que no ayuda a nadie a saber que
    /// version le falta.
    pub fn tipo_original(&self) -> Option<&str> {
        match self {
            Action::Unknown { raw } => raw.get("type").and_then(|v| v.as_str()),
            _ => None,
        }
    }
}
