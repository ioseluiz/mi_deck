//! Modelo de datos del deck.
//!
//! La estructura es un registro plano de superficies con referencias, no un arbol
//! anidado: una superficie es un nivel navegable (la raiz, o el interior de una
//! carpeta) y un boton de tipo `folder` apunta a otra superficie por id. Mover una
//! carpeta de sitio es cambiar una linea, no recortar y pegar un bloque anidado.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub grid: Grid,
    pub key_size: u32,
    pub always_on_top: bool,
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
            always_on_top: true,
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
    #[serde(default)]
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
    #[serde(default)]
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
