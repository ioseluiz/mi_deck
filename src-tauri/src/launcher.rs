//! Ejecucion de las acciones de una tecla.
//!
//! Partido en dos a proposito:
//!   - build_launch() decide QUE lanzar: expande variables, parte argumentos y
//!     elige el mecanismo. Es determinista y esta cubierta por tests.
//!   - execute() solo lanza. Es tan delgada que no hay donde esconder un bug.
//!
//! Todo se lanza desacoplado: cerrar el widget no debe matar lo que abrio.

use std::path::Path;

use crate::model::{Action, Shell};

/// Bandera de CreateProcess: no crear ventana de consola para el hijo.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x0000_0008;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchSpec {
    /// Lanzar un ejecutable directamente.
    Process {
        program: String,
        args: Vec<String>,
        workdir: Option<String>,
        no_window: bool,
    },
    /// Delegar en el shell de Windows: resuelve .lnk, carpetas y URLs, y respeta
    /// la app predeterminada del usuario.
    Shell { target: String },
    /// Abrir el Explorador con un archivo seleccionado.
    Reveal { path: String },
    /// No lanza nada: lo resuelve la navegacion del propio deck.
    Navigate { surface: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// El destino esta vacio en deck.json.
    Empty { kind: &'static str },
    /// La ruta no existe. Se devuelve con la ruta ya expandida, que es la que el
    /// usuario necesita ver para corregirla.
    NotFound { target: String },
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::Empty { kind } => {
                write!(f, "La accion de tipo {kind} no tiene destino configurado.")
            }
            LaunchError::NotFound { target } => {
                if target.contains(['\\', '/']) {
                    write!(f, "No se encontro: {target}")
                } else {
                    // Un nombre suelto que no se resolvio: lo util es decir donde
                    // se busco y como arreglarlo, no solo que no aparecio.
                    write!(
                        f,
                        "No se encontro {target}: no esta en el PATH ni registrado                          en Windows. Pon la ruta completa al ejecutable."
                    )
                }
            }
        }
    }
}

impl std::error::Error for LaunchError {}

// ----------------------------------------------------------------- expansion

/// Expande %VARIABLES% de Windows. Una variable inexistente se deja tal cual, que
/// es mas util que borrarla: el mensaje de error muestra que fallo.
pub fn expand_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == '%' {
            if let Some(cierre) = bytes[i + 1..].iter().position(|c| *c == '%') {
                let nombre: String = bytes[i + 1..i + 1 + cierre].iter().collect();
                if !nombre.is_empty() {
                    if let Ok(valor) = std::env::var(&nombre) {
                        out.push_str(&valor);
                        i += cierre + 2;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Parte una linea de argumentos respetando comillas dobles.
/// `--perfil "Profile 1" -x` da tres argumentos, no cuatro.
pub fn split_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut actual = String::new();
    let mut en_comillas = false;
    let mut hay_token = false;

    for c in input.chars() {
        match c {
            '"' => {
                en_comillas = !en_comillas;
                hay_token = true;
            }
            c if c.is_whitespace() && !en_comillas => {
                if hay_token {
                    args.push(std::mem::take(&mut actual));
                    hay_token = false;
                }
            }
            c => {
                actual.push(c);
                hay_token = true;
            }
        }
    }
    if hay_token {
        args.push(actual);
    }
    args
}

/// Resuelve un destino a una ruta real en disco.
///
/// Si ya es una ruta existente, la devuelve. Si es un nombre suelto como
/// "notepad.exe", lo busca en PATH, igual que haria el shell. Hace falta tanto
/// para validar antes de lanzar como para extraer el icono del ejecutable.
pub fn resolve_program(target: &str) -> Option<std::path::PathBuf> {
    let p = Path::new(target);
    if p.exists() {
        return Some(p.to_path_buf());
    }
    // Si traia separadores, era una ruta concreta y simplemente no existe.
    if target.contains('\\') || target.contains('/') {
        return None;
    }

    let extensiones: Vec<String> = if p.extension().is_some() {
        vec![String::new()]
    } else {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.COM;.BAT;.CMD".to_string())
            .split(';')
            .map(|e| e.to_string())
            .collect()
    };

    let rutas = std::env::var("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&rutas) {
        for ext in &extensiones {
            let candidato = dir.join(format!("{target}{ext}"));
            if candidato.is_file() {
                return Some(candidato);
            }
        }
    }

    // Ultimo recurso: App Paths del registro. Los navegadores no estan en PATH,
    // pero si registrados ahi.
    for ext in &extensiones {
        if let Some(p) = desde_app_paths(&format!("{target}{ext}")) {
            return Some(p);
        }
    }
    None
}

/// Ruta registrada en App Paths para un nombre de ejecutable.
///
/// Windows resuelve nombres sueltos como "chrome.exe" por esta clave del
/// registro, no por PATH: es lo que hace que funcione Win+R. ShellExecute la
/// consulta sola, pero CreateProcess no, y abrir una URL con un navegador
/// concreto pasa por CreateProcess. Sin esto, "edge" y "chrome" no se
/// encontraban nunca aunque estuvieran instalados.
#[cfg(windows)]
fn desde_app_paths(nombre: &str) -> Option<std::path::PathBuf> {
    const APP_PATHS: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths";
    const APP_PATHS_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths";

    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    // El usuario manda sobre la maquina; la vista de 32 bits va al final porque
    // es donde acaban las instalaciones antiguas.
    let sitios = [
        (HKEY_CURRENT_USER, APP_PATHS),
        (HKEY_LOCAL_MACHINE, APP_PATHS),
        (HKEY_LOCAL_MACHINE, APP_PATHS_32),
    ];

    for (raiz, base) in sitios {
        if let Some(valor) = leer_registro(raiz, &format!("{base}\\{nombre}")) {
            let ruta = std::path::PathBuf::from(limpiar_ruta(&valor));
            if ruta.is_file() {
                return Some(ruta);
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn desde_app_paths(_nombre: &str) -> Option<std::path::PathBuf> {
    None
}

/// Valor predeterminado (sin nombre) de una clave del registro.
#[cfg(windows)]
fn leer_registro(raiz: windows::Win32::System::Registry::HKEY, clave: &str) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_SZ};

    let clave_w: Vec<u16> = std::ffi::OsStr::new(clave)
        .encode_wide()
        .chain(Some(0))
        .collect();

    let mut bytes: u32 = 0;
    unsafe {
        // Primera llamada sin buffer: solo para saber cuanto ocupa.
        if RegGetValueW(
            raiz,
            PCWSTR(clave_w.as_ptr()),
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
        .is_err()
            || bytes == 0
        {
            return None;
        }

        let mut buffer = vec![0u16; (bytes as usize / 2) + 1];
        let mut tam = bytes;
        if RegGetValueW(
            raiz,
            PCWSTR(clave_w.as_ptr()),
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr() as *mut _),
            Some(&mut tam),
        )
        .is_err()
        {
            return None;
        }

        let largo = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..largo]))
    }
}

/// El valor de App Paths suele venir entrecomillado.
fn limpiar_ruta(valor: &str) -> String {
    valor.trim().trim_matches('"').trim().to_string()
}

// --------------------------------------------------------------- build_launch

pub fn build_launch(action: &Action) -> Result<LaunchSpec, LaunchError> {
    match action {
        Action::Folder { surface } => Ok(LaunchSpec::Navigate {
            surface: surface.clone(),
        }),

        Action::App {
            target,
            args,
            workdir,
            ..
        } => {
            let target = expand_env(target);
            if target.trim().is_empty() {
                return Err(LaunchError::Empty { kind: "app" });
            }
            let args_v = split_args(&expand_env(args));

            // Un .lnk no lo resuelve CreateProcess, y un destino que no es un
            // ejecutable (un .docx, un .pdf) tiene que abrirlo su app predeterminada:
            // ambos casos pasan por el shell. Solo un ejecutable va por CreateProcess.
            if !parece_ejecutable(&target) {
                return Ok(LaunchSpec::Shell { target });
            }

            let workdir = expand_env(workdir);
            Ok(LaunchSpec::Process {
                program: target,
                args: args_v,
                workdir: (!workdir.trim().is_empty()).then_some(workdir),
                no_window: true,
            })
        }

        Action::Url {
            target,
            browser,
            profile,
        } => {
            let target = expand_env(target);
            if target.trim().is_empty() {
                return Err(LaunchError::Empty { kind: "url" });
            }
            if browser.eq_ignore_ascii_case("default") || browser.trim().is_empty() {
                return Ok(LaunchSpec::Shell { target });
            }

            let mut args = Vec::new();
            if let Some(perfil) = profile {
                if !perfil.trim().is_empty() {
                    args.push(format!("--profile-directory={perfil}"));
                }
            }
            args.push(target);

            Ok(LaunchSpec::Process {
                program: programa_de_navegador(browser),
                args,
                workdir: None,
                no_window: true,
            })
        }

        Action::Path { target } => {
            let target = expand_env(target);
            if target.trim().is_empty() {
                return Err(LaunchError::Empty { kind: "path" });
            }
            let p = Path::new(&target);
            // Un archivo se revela seleccionado en el Explorador; una carpeta se abre.
            if p.is_file() {
                Ok(LaunchSpec::Reveal { path: target })
            } else {
                Ok(LaunchSpec::Shell { target })
            }
        }

        Action::Script {
            shell,
            target,
            args,
            hidden,
        } => {
            let target = expand_env(target);
            if target.trim().is_empty() {
                return Err(LaunchError::Empty { kind: "script" });
            }
            let extra = split_args(&expand_env(args));
            let bajo = target.to_ascii_lowercase();

            let (program, mut argv) = match shell {
                Shell::Powershell => {
                    let mut v = vec![
                        "-NoProfile".to_string(),
                        "-ExecutionPolicy".to_string(),
                        "Bypass".to_string(),
                    ];
                    if *hidden {
                        v.push("-WindowStyle".to_string());
                        v.push("Hidden".to_string());
                    }
                    if bajo.ends_with(".ps1") {
                        v.push("-File".to_string());
                        v.push(target.clone());
                    } else {
                        // No es un archivo: se trata como comando suelto.
                        v.push("-Command".to_string());
                        v.push(target.clone());
                    }
                    ("powershell.exe".to_string(), v)
                }
                Shell::Cmd => {
                    let v = vec!["/c".to_string(), target.clone()];
                    ("cmd.exe".to_string(), v)
                }
            };
            argv.extend(extra);

            Ok(LaunchSpec::Process {
                program,
                args: argv,
                workdir: None,
                no_window: *hidden,
            })
        }
    }
}

/// Un nombre suelto como "notepad.exe" lo resuelve el PATH; una ruta con
/// separadores tiene que existir.
fn parece_ejecutable(target: &str) -> bool {
    let bajo = target.to_ascii_lowercase();
    bajo.ends_with(".exe")
        || bajo.ends_with(".com")
        || bajo.ends_with(".bat")
        || bajo.ends_with(".cmd")
}

fn programa_de_navegador(browser: &str) -> String {
    match browser.to_ascii_lowercase().as_str() {
        "edge" | "msedge" => "msedge.exe".to_string(),
        "chrome" => "chrome.exe".to_string(),
        "firefox" => "firefox.exe".to_string(),
        otro => otro.to_string(), // ruta completa a un .exe
    }
}

// -------------------------------------------------------------- verificacion

/// Comprueba que el destino existe antes de lanzar, para poder marcar la tecla y
/// mostrar la ruta exacta en vez de fallar en silencio.
pub fn validate(spec: &LaunchSpec) -> Result<(), LaunchError> {
    match spec {
        LaunchSpec::Navigate { .. } => Ok(()),
        LaunchSpec::Reveal { path } => existe(path),
        LaunchSpec::Shell { target } => {
            // Una URL no se comprueba contra el disco.
            if es_url(target) {
                Ok(())
            } else {
                existe(target)
            }
        }
        LaunchSpec::Process { program, .. } => {
            // Se comprueba tambien el PATH, asi un nombre mal escrito se detecta
            // antes de lanzar y no acaba en un fallo opaco de CreateProcess.
            if resolve_program(program).is_some() {
                Ok(())
            } else {
                Err(LaunchError::NotFound {
                    target: program.to_string(),
                })
            }
        }
    }
}

fn existe(target: &str) -> Result<(), LaunchError> {
    if Path::new(target).exists() {
        Ok(())
    } else {
        Err(LaunchError::NotFound {
            target: target.to_string(),
        })
    }
}

fn es_url(target: &str) -> bool {
    let bajo = target.to_ascii_lowercase();
    bajo.starts_with("http://")
        || bajo.starts_with("https://")
        || bajo.starts_with("mailto:")
        || bajo.starts_with("ms-")
}

// ------------------------------------------------------------------ ejecucion

/// Ruta con la que invocar realmente a un programa.
///
/// Si el nombre se puede resolver (PATH o App Paths), se usa la ruta completa.
/// Si no, se deja tal cual para que el error del sistema sea el que se muestre.
pub fn programa_a_lanzar(program: &str) -> String {
    resolve_program(program)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| program.to_string())
}

/// Lanza lo que describe el spec. Capa delgada a proposito.
pub fn execute(spec: &LaunchSpec) -> Result<(), String> {
    validate(spec).map_err(|e| e.to_string())?;

    match spec {
        LaunchSpec::Navigate { .. } => Ok(()),

        LaunchSpec::Process {
            program,
            args,
            workdir,
            no_window,
        } => {
            // CreateProcess tampoco consulta App Paths, asi que no basta con
            // haberlo validado: hay que entregarle la ruta ya resuelta o un
            // "chrome.exe" suelto falla con "program not found".
            let ejecutable = programa_a_lanzar(program);
            let mut cmd = std::process::Command::new(&ejecutable);
            cmd.args(args);
            if let Some(dir) = workdir {
                cmd.current_dir(dir);
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let mut flags = DETACHED_PROCESS;
                if *no_window {
                    flags |= CREATE_NO_WINDOW;
                }
                cmd.creation_flags(flags);
            }
            cmd.spawn()
                .map(|_| ())
                .map_err(|e| format!("No se pudo lanzar {program}: {e}"))
        }

        LaunchSpec::Shell { target } => open_with_shell(target),

        LaunchSpec::Reveal { path } => {
            // explorer.exe exige /select,<ruta> como UN solo argumento.
            std::process::Command::new("explorer.exe")
                .arg(format!("/select,{path}"))
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("No se pudo abrir el Explorador en {path}: {e}"))
        }
    }
}

fn open_with_shell(target: &str) -> Result<(), String> {
    tauri_plugin_opener::open_path(target, None::<&str>)
        .map_err(|e| format!("No se pudo abrir {target}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, Shell};

    fn app(target: &str, args: &str) -> Action {
        Action::App {
            target: target.into(),
            args: args.into(),
            workdir: String::new(),
            focus_if_running: false,
        }
    }

    // --------------------------------------------------------------- utilidades

    #[test]
    fn expande_variables_de_entorno() {
        std::env::set_var("MIDECK_PRUEBA", "C:\\Datos");
        assert_eq!(expand_env("%MIDECK_PRUEBA%\\x.txt"), "C:\\Datos\\x.txt");
    }

    #[test]
    fn una_variable_inexistente_se_deja_visible() {
        // Borrarla silenciosamente dejaria una ruta rota e inexplicable.
        assert_eq!(expand_env("%NO_EXISTE_JAMAS%\\x"), "%NO_EXISTE_JAMAS%\\x");
    }

    #[test]
    fn parte_argumentos_respetando_comillas() {
        assert_eq!(
            split_args("--perfil \"Profile 1\" -x"),
            vec!["--perfil", "Profile 1", "-x"]
        );
        assert_eq!(split_args("   "), Vec::<String>::new());
        assert_eq!(
            split_args("\"C:\\Con Espacios\\a.txt\""),
            vec!["C:\\Con Espacios\\a.txt"]
        );
    }

    // ------------------------------------------------------------------- app

    #[test]
    fn un_exe_con_argumentos_se_lanza_como_proceso() {
        let spec = build_launch(&app("C:\\Win\\app.exe", "--flag \"dos palabras\"")).unwrap();
        assert_eq!(
            spec,
            LaunchSpec::Process {
                program: "C:\\Win\\app.exe".into(),
                args: vec!["--flag".into(), "dos palabras".into()],
                workdir: None,
                no_window: true,
            }
        );
    }

    #[test]
    fn un_acceso_directo_pasa_siempre_por_el_shell() {
        // CreateProcess no resuelve .lnk; ShellExecute si.
        let spec = build_launch(&app("C:\\Escritorio\\App.lnk", "--con-args")).unwrap();
        assert_eq!(
            spec,
            LaunchSpec::Shell {
                target: "C:\\Escritorio\\App.lnk".into()
            }
        );
    }

    #[test]
    fn un_destino_vacio_da_error_en_vez_de_lanzar_nada() {
        assert_eq!(
            build_launch(&app("   ", "")),
            Err(LaunchError::Empty { kind: "app" })
        );
    }

    // ------------------------------------------------------------------- url

    #[test]
    fn una_url_con_navegador_predeterminado_va_al_shell() {
        let a = Action::Url {
            target: "https://pancanal.com".into(),
            browser: "default".into(),
            profile: None,
        };
        assert_eq!(
            build_launch(&a).unwrap(),
            LaunchSpec::Shell {
                target: "https://pancanal.com".into()
            }
        );
    }

    #[test]
    fn una_url_con_perfil_de_edge_arma_el_argumento_correcto() {
        let a = Action::Url {
            target: "https://pancanal.com".into(),
            browser: "edge".into(),
            profile: Some("Profile 2".into()),
        };
        assert_eq!(
            build_launch(&a).unwrap(),
            LaunchSpec::Process {
                program: "msedge.exe".into(),
                args: vec![
                    "--profile-directory=Profile 2".into(),
                    "https://pancanal.com".into()
                ],
                workdir: None,
                no_window: true,
            }
        );
    }

    #[test]
    fn una_url_no_se_valida_contra_el_disco() {
        let spec = LaunchSpec::Shell {
            target: "https://pancanal.com".into(),
        };
        assert!(validate(&spec).is_ok());
    }

    // ------------------------------------------------------------------ path

    #[test]
    fn una_carpeta_se_abre_y_un_archivo_se_revela_seleccionado() {
        let dir = std::env::temp_dir().join("mideck-launcher-path");
        let _ = std::fs::create_dir_all(&dir);
        let archivo = dir.join("hola.txt");
        std::fs::write(&archivo, "x").unwrap();

        let carpeta_spec = build_launch(&Action::Path {
            target: dir.to_string_lossy().to_string(),
        })
        .unwrap();
        assert!(matches!(carpeta_spec, LaunchSpec::Shell { .. }));

        let archivo_spec = build_launch(&Action::Path {
            target: archivo.to_string_lossy().to_string(),
        })
        .unwrap();
        assert!(matches!(archivo_spec, LaunchSpec::Reveal { .. }));
    }

    #[test]
    fn una_ruta_inexistente_falla_con_la_ruta_a_la_vista() {
        let spec = LaunchSpec::Shell {
            target: "C:\\no\\existe\\jamas.txt".into(),
        };
        assert_eq!(
            validate(&spec),
            Err(LaunchError::NotFound {
                target: "C:\\no\\existe\\jamas.txt".into()
            })
        );
    }

    #[test]
    fn un_nombre_suelto_se_busca_en_el_path() {
        let ok = LaunchSpec::Process {
            program: "notepad.exe".into(),
            args: vec![],
            workdir: None,
            no_window: true,
        };
        assert!(validate(&ok).is_ok());

        // Un nombre mal escrito se detecta antes de lanzar, en vez de acabar en un
        // fallo opaco de CreateProcess.
        let mal = LaunchSpec::Process {
            program: "notpad.exe".into(),
            args: vec![],
            workdir: None,
            no_window: true,
        };
        assert!(matches!(validate(&mal), Err(LaunchError::NotFound { .. })));
    }

    #[test]
    fn limpia_las_comillas_del_valor_del_registro() {
        // App Paths suele guardar la ruta entrecomillada.
        assert_eq!(
            limpiar_ruta(r#""C:\Program Files\Google\Chrome\Application\chrome.exe""#),
            r"C:\Program Files\Google\Chrome\Application\chrome.exe"
        );
        assert_eq!(limpiar_ruta(r"  C:\x\y.exe  "), r"C:\x\y.exe");
    }

    #[cfg(windows)]
    #[test]
    fn al_lanzar_un_navegador_se_usa_su_ruta_completa() {
        // CreateProcess no resuelve nombres sueltos por App Paths: si se le pasa
        // "chrome.exe" falla con "program not found" aunque este instalado.
        for navegador in ["msedge.exe", "chrome.exe"] {
            if desde_app_paths(navegador).is_none() {
                continue; // no instalado en este equipo
            }
            let ruta = programa_a_lanzar(navegador);
            assert_ne!(ruta, navegador, "{navegador} se paso sin resolver");
            assert!(
                std::path::Path::new(&ruta).is_file(),
                "{navegador} resolvio a algo que no existe: {ruta}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn encuentra_los_navegadores_aunque_no_esten_en_el_path() {
        // Edge y Chrome no se anaden al PATH: Windows los resuelve por App Paths
        // del registro. Sin consultarla, abrir una URL con un navegador concreto
        // fallaba siempre con "no se encontro msedge.exe".
        for navegador in ["msedge.exe", "chrome.exe"] {
            let en_path = std::env::var("PATH")
                .unwrap_or_default()
                .split(';')
                .any(|d| std::path::Path::new(d).join(navegador).is_file());
            assert!(
                !en_path,
                "{navegador} esta en PATH: la premisa del test cambio"
            );

            // Solo se exige si el navegador esta instalado en este equipo.
            if desde_app_paths(navegador).is_some() {
                let r = resolve_program(navegador);
                assert!(
                    r.is_some(),
                    "{navegador} esta registrado pero no se resolvio"
                );
                assert!(r.unwrap().is_file());
            }
        }
    }

    #[test]
    fn resolve_program_encuentra_los_ejecutables_del_sistema() {
        let p = resolve_program("notepad.exe").expect("notepad debe estar en PATH");
        assert!(p.is_file());
        // Sin extension tambien, probando las de PATHEXT.
        assert!(resolve_program("notepad").is_some());
        assert!(resolve_program("no_existe_este_programa_jamas").is_none());
        // Una ruta concreta inexistente no se busca en PATH.
        assert!(resolve_program("C:\\no\\existe\\notepad.exe").is_none());
    }

    // ---------------------------------------------------------------- script

    #[test]
    fn un_ps1_oculto_lleva_file_y_windowstyle_hidden() {
        let a = Action::Script {
            shell: Shell::Powershell,
            target: "C:\\s\\respaldo.ps1".into(),
            args: "-Destino D:\\".into(),
            hidden: true,
        };
        let spec = build_launch(&a).unwrap();
        assert_eq!(
            spec,
            LaunchSpec::Process {
                program: "powershell.exe".into(),
                args: vec![
                    "-NoProfile".into(),
                    "-ExecutionPolicy".into(),
                    "Bypass".into(),
                    "-WindowStyle".into(),
                    "Hidden".into(),
                    "-File".into(),
                    "C:\\s\\respaldo.ps1".into(),
                    "-Destino".into(),
                    "D:\\".into(),
                ],
                workdir: None,
                no_window: true,
            }
        );
    }

    #[test]
    fn un_comando_suelto_usa_command_y_no_file() {
        let a = Action::Script {
            shell: Shell::Powershell,
            target: "Get-Date".into(),
            args: String::new(),
            hidden: false,
        };
        let LaunchSpec::Process {
            args, no_window, ..
        } = build_launch(&a).unwrap()
        else {
            panic!("deberia ser un proceso");
        };
        assert!(args.contains(&"-Command".to_string()));
        assert!(!args.contains(&"-File".to_string()));
        // Sin hidden hay consola visible: es lo que el usuario quiere para ver salida.
        assert!(!no_window);
        assert!(!args.contains(&"Hidden".to_string()));
    }

    #[test]
    fn cmd_usa_barra_c() {
        let a = Action::Script {
            shell: Shell::Cmd,
            target: "C:\\s\\tarea.bat".into(),
            args: String::new(),
            hidden: true,
        };
        let spec = build_launch(&a).unwrap();
        assert_eq!(
            spec,
            LaunchSpec::Process {
                program: "cmd.exe".into(),
                args: vec!["/c".into(), "C:\\s\\tarea.bat".into()],
                workdir: None,
                no_window: true,
            }
        );
    }

    // ---------------------------------------------------------------- folder

    #[test]
    fn una_carpeta_del_deck_no_lanza_nada() {
        let a = Action::Folder {
            surface: "s-proy".into(),
        };
        assert_eq!(
            build_launch(&a).unwrap(),
            LaunchSpec::Navigate {
                surface: "s-proy".into()
            }
        );
    }
}
