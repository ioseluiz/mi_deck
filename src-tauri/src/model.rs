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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowPos>,
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
            window: None,
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

/// Enum etiquetado: el compilador obliga a cubrir los cinco tipos en cada `match`,
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
}

fn default_browser() -> String {
    "default".to_string()
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
            Action::Path { .. } => "path",
            Action::Script { .. } => "script",
            Action::Folder { .. } => "folder",
        }
    }
}
