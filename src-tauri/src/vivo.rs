//! Lo que una tecla puede ensenar del sistema sin que la pulses.
//!
//! Es la diferencia de fondo con una consola de hardware: alli cada tecla es una
//! pantallita que dice el volumen, si el micro esta en rojo o si la aplicacion
//! esta abierta. Aqui, hasta ahora, una tecla se veia siempre igual.
//!
//! El modulo esta partido en dos a proposito. `leer` toca Windows y se hace **una
//! vez por latido**, por muchas teclas que pidan lo mismo; `valor_de` es pura y
//! convierte esas lecturas en lo que se pinta, asi que todo el formateo --que es
//! donde estan los casos raros-- se prueba sin encender nada.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// De donde saca una tecla lo que ensena.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Fuente {
    BloqMayus,
    BloqNum,
    /// Si hay algun proceso con este ejecutable: "teams.exe".
    AppAbierta {
        exe: String,
    },
    Reloj,
    /// Volumen del altavoz, de 0 a 100.
    Volumen,
    /// Si el altavoz esta silenciado.
    AltavozSilenciado,
    /// Si el microfono esta silenciado.
    MicroSilenciado,
    Bateria,
    MemoriaUsada,
    /// Espacio libre de una unidad: "C:".
    DiscoLibre {
        unidad: String,
    },
    /// Una fuente que esta version no conoce.
    ///
    /// Sin esta variante, un `live` escrito por una version mas nueva no encaja
    /// con el enum, la tecla entera deja de leerse y con ella **el deck
    /// completo**: se da por corrupto, se respalda y se arranca de cero. Lo
    /// encontro un test al anadir el campo. Es el mismo accidente que
    /// `Action::Unknown` evita, y se resuelve igual.
    Desconocida {
        #[serde(rename = "__original")]
        raw: serde_json::Map<String, serde_json::Value>,
    },
}

/// Lee una fuente sin que una desconocida se lleve la tecla por delante.
///
/// Mismo reparto que `accion_tolerante`: lo que no se entiende se envuelve tal
/// cual y vuelve al disco intacto, asi que una version que si lo entienda lo
/// recupera.
pub fn fuente_tolerante<'de, D>(d: D) -> Result<Option<Fuente>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let valor = Option::<serde_json::Value>::deserialize(d)?;
    let Some(valor) = valor else {
        return Ok(None);
    };

    match serde_json::from_value::<Fuente>(valor.clone()) {
        Ok(Fuente::Desconocida { raw }) => {
            let interior = serde_json::Value::Object(raw.clone());
            match serde_json::from_value::<Fuente>(interior) {
                Ok(Fuente::Desconocida { .. }) | Err(_) => Ok(Some(Fuente::Desconocida { raw })),
                Ok(recuperada) => Ok(Some(recuperada)),
            }
        }
        Ok(conocida) => Ok(Some(conocida)),
        Err(_) => Ok(Some(Fuente::Desconocida {
            raw: match valor {
                serde_json::Value::Object(m) => m,
                otro => {
                    let mut m = serde_json::Map::new();
                    m.insert("__valor".to_string(), otro);
                    m
                }
            },
        })),
    }
}

/// Lo que una fuente da para pintar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Valor {
    /// Lo que se escribe en la esquina de la tecla. Vacio si no hay nada que decir.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texto: Option<String>,
    /// Encendido o apagado, para las fuentes que son un si o un no.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encendido: Option<bool>,
}

impl Valor {
    fn texto(t: impl Into<String>) -> Self {
        Self {
            texto: Some(t.into()),
            encendido: None,
        }
    }
    fn interruptor(on: bool) -> Self {
        Self {
            texto: None,
            encendido: Some(on),
        }
    }
    /// Lo que no se pudo leer: sin portatil no hay bateria, y una unidad de red
    /// caida no tiene espacio libre. Mejor no decir nada que decir un cero.
    fn nada() -> Self {
        Self {
            texto: None,
            encendido: None,
        }
    }
}

/// Todo lo que hizo falta preguntarle a Windows en este latido.
///
/// Se recoge de golpe para que veinte teclas de reloj no sean veinte llamadas, y
/// para que `valor_de` no tenga que tocar el sistema.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lecturas {
    pub bloq_mayus: bool,
    pub bloq_num: bool,
    /// Hora local: hora y minuto. Sin segundos: una tecla que parpadea cada
    /// segundo es ruido, y obligaria a latir mas deprisa de lo necesario.
    pub hora: Option<(u16, u16)>,
    pub volumen: Option<u8>,
    pub altavoz_silenciado: Option<bool>,
    pub micro_silenciado: Option<bool>,
    pub bateria: Option<u8>,
    pub memoria_usada: Option<u8>,
    /// Ejecutables con algun proceso vivo, en minusculas.
    pub procesos: Vec<String>,
    /// Bytes libres por unidad, solo de las que alguna tecla pidio.
    pub discos: HashMap<String, u64>,
}

/// Lo que ensena una tecla, a partir de lo ya leido. Pura.
pub fn valor_de(fuente: &Fuente, l: &Lecturas) -> Valor {
    match fuente {
        Fuente::BloqMayus => Valor::interruptor(l.bloq_mayus),
        Fuente::BloqNum => Valor::interruptor(l.bloq_num),

        Fuente::AppAbierta { exe } => {
            let clave = crate::focus::nombre_de_ejecutable(exe);
            Valor::interruptor(l.procesos.iter().any(|p| p.eq_ignore_ascii_case(&clave)))
        }

        Fuente::Volumen => match l.volumen {
            Some(p) => Valor {
                texto: Some(format!("{p}%")),
                // Apagada al silencio, no al cero: silenciar y bajar del todo son
                // cosas distintas en Windows y la tecla tiene que distinguirlas.
                encendido: l.altavoz_silenciado.map(|m| !m),
            },
            None => Valor::nada(),
        },

        // Silenciado es la cara "alterada", asi que encendido significa que se
        // oye: la cara 0 es siempre el estado normal.
        Fuente::AltavozSilenciado => match l.altavoz_silenciado {
            Some(m) => Valor::interruptor(!m),
            None => Valor::nada(),
        },
        Fuente::MicroSilenciado => match l.micro_silenciado {
            Some(m) => Valor::interruptor(!m),
            None => Valor::nada(),
        },

        Fuente::Reloj => match l.hora {
            Some((h, m)) => Valor::texto(format!("{h:02}:{m:02}")),
            None => Valor::nada(),
        },

        Fuente::Bateria => match l.bateria {
            Some(p) => Valor {
                texto: Some(format!("{p}%")),
                // Por debajo de un quinto se considera apagada, para que una tecla
                // de dos caras pueda cambiar de cara cuando queda poca.
                encendido: Some(p > 20),
            },
            None => Valor::nada(),
        },

        Fuente::MemoriaUsada => match l.memoria_usada {
            Some(p) => Valor::texto(format!("{p}%")),
            None => Valor::nada(),
        },

        Fuente::DiscoLibre { unidad } => match l.discos.get(&normalizar_unidad(unidad)) {
            Some(bytes) => Valor::texto(tamano_corto(*bytes)),
            None => Valor::nada(),
        },

        // Una fuente de una version mas nueva: la tecla funciona, simplemente no
        // ensena nada. Mejor eso que perder la tecla.
        Fuente::Desconocida { .. } => Valor::nada(),
    }
}

/// "c", "C:", "c:/" y "C:\\" son la misma unidad. Pura.
pub fn normalizar_unidad(u: &str) -> String {
    let letra = u.chars().next().unwrap_or('c');
    format!("{}:", letra.to_ascii_uppercase())
}

/// Bytes en algo que quepa en la esquina de una tecla. Pura.
///
/// Sin decimales a partir de diez: "128 GB" cabe donde "128,4 GB" no, y el medio
/// giga de diferencia no le importa a nadie que mira una tecla de reojo.
pub fn tamano_corto(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let gb = bytes as f64 / GB;
    if gb >= 1024.0 {
        format!("{:.1} TB", gb / 1024.0)
    } else if gb >= 10.0 {
        format!("{} GB", gb.round() as u64)
    } else {
        format!("{gb:.1} GB")
    }
}

/// Solo lo que alguna tecla pide, para no preguntar de mas en cada latido.
pub fn leer(fuentes: &[Fuente]) -> Lecturas {
    let mut l = Lecturas::default();
    if fuentes.is_empty() {
        return l;
    }

    let quiere = |f: &Fuente| fuentes.contains(f);
    if quiere(&Fuente::BloqMayus) {
        l.bloq_mayus = tecla_activa(0x14);
    }
    if quiere(&Fuente::BloqNum) {
        l.bloq_num = tecla_activa(0x90);
    }
    if quiere(&Fuente::Reloj) {
        l.hora = hora_local();
    }
    if quiere(&Fuente::Volumen) {
        l.volumen = crate::audio::volumen(crate::audio::Flujo::Altavoz);
    }
    if quiere(&Fuente::Volumen) || quiere(&Fuente::AltavozSilenciado) {
        l.altavoz_silenciado = crate::audio::silenciado(crate::audio::Flujo::Altavoz);
    }
    if quiere(&Fuente::MicroSilenciado) {
        l.micro_silenciado = crate::audio::silenciado(crate::audio::Flujo::Microfono);
    }
    if quiere(&Fuente::Bateria) {
        l.bateria = bateria();
    }
    if quiere(&Fuente::MemoriaUsada) {
        l.memoria_usada = memoria_usada();
    }
    if fuentes
        .iter()
        .any(|f| matches!(f, Fuente::AppAbierta { .. }))
    {
        l.procesos = procesos_cacheados();
    }
    for f in fuentes {
        if let Fuente::DiscoLibre { unidad } = f {
            let u = normalizar_unidad(unidad);
            if let Some(libre) = disco_libre(&u) {
                l.discos.insert(u, libre);
            }
        }
    }
    l
}

// --------------------------------------------------------------- Windows

/// Si una tecla de bloqueo esta activada.
///
/// El bit bajo de `GetKeyState` es el que dice si esta "encendida", a diferencia
/// del alto, que dice si esta pulsada ahora mismo.
#[cfg(windows)]
fn tecla_activa(vk: i32) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
    unsafe { GetKeyState(vk) & 1 != 0 }
}

#[cfg(not(windows))]
fn tecla_activa(_vk: i32) -> bool {
    false
}

#[cfg(windows)]
fn hora_local() -> Option<(u16, u16)> {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe { GetLocalTime() };
    Some((t.wHour, t.wMinute))
}

#[cfg(not(windows))]
fn hora_local() -> Option<(u16, u16)> {
    None
}

/// Porcentaje de bateria, o None en un equipo sin ella.
#[cfg(windows)]
fn bateria() -> Option<u8> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    let mut s = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
        return None;
    }
    // 255 es "no se sabe", que es lo que devuelve un equipo de sobremesa.
    (s.BatteryLifePercent != 255).then_some(s.BatteryLifePercent)
}

#[cfg(not(windows))]
fn bateria() -> Option<u8> {
    None
}

#[cfg(windows)]
fn memoria_usada() -> Option<u8> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut m = MEMORYSTATUSEX {
        dwLength: core::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut m) }.ok()?;
    Some(m.dwMemoryLoad.min(100) as u8)
}

#[cfg(not(windows))]
fn memoria_usada() -> Option<u8> {
    None
}

#[cfg(windows)]
fn disco_libre(unidad: &str) -> Option<u64> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let raiz = HSTRING::from(format!("{unidad}\\"));
    let mut libre = 0u64;
    unsafe { GetDiskFreeSpaceExW(&raiz, None, None, Some(&mut libre)) }.ok()?;
    Some(libre)
}

#[cfg(not(windows))]
fn disco_libre(_unidad: &str) -> Option<u64> {
    None
}

/// Cada cuanto se vuelve a mirar la lista de procesos.
///
/// Medido en este equipo: el latido entero cuesta 16 ms cada 30 s con una tecla de
/// reloj y 0 con una de disco, pero **344 ms con una de aplicacion abierta**.
/// Enumerar todos los procesos del sistema es, con diferencia, lo mas caro que
/// hace MiDeck de forma recurrente, y «esta abierto Teams» no necesita resolucion
/// de un segundo. A cuatro, el gasto baja a la cuarta parte y lo unico que se
/// pierde es que abrir o cerrar algo tarde hasta cuatro segundos en notarse.
const REFRESCO_PROCESOS: std::time::Duration = std::time::Duration::from_secs(4);

/// Lista de procesos, reaprovechada entre latidos.
fn procesos_cacheados() -> Vec<String> {
    use std::sync::Mutex;
    use std::time::Instant;

    static CACHE: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);

    let mut guardado = CACHE.lock().unwrap();
    if let Some((cuando, lista)) = guardado.as_ref() {
        if cuando.elapsed() < REFRESCO_PROCESOS {
            return lista.clone();
        }
    }
    let lista = procesos();
    *guardado = Some((Instant::now(), lista.clone()));
    lista
}

/// Nombres de ejecutable con algun proceso vivo, en minusculas.
///
/// Por procesos y no por ventanas a proposito. Enumerar ventanas es mas barato y
/// `focus::apps_en_ejecucion` ya lo hace, pero responde "tiene ventana visible":
/// Teams u Outlook minimizados a la bandeja saldrian como cerrados, que es justo
/// cuando uno mira la tecla para saber si estan.
#[cfg(windows)]
fn procesos() -> Vec<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut v = Vec::new();
    unsafe {
        let Ok(foto) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return v;
        };
        let mut e = PROCESSENTRY32W {
            dwSize: core::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(foto, &mut e).is_ok() {
            loop {
                let largo = e
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(e.szExeFile.len());
                let nombre = String::from_utf16_lossy(&e.szExeFile[..largo]).to_lowercase();
                if !nombre.is_empty() {
                    v.push(nombre);
                }
                if Process32NextW(foto, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(foto);
    }
    v.sort();
    v.dedup();
    v
}

#[cfg(not(windows))]
fn procesos() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn con(l: Lecturas) -> Lecturas {
        l
    }

    #[test]
    fn las_teclas_de_bloqueo_son_un_si_o_un_no_sin_texto() {
        let l = con(Lecturas {
            bloq_mayus: true,
            ..Default::default()
        });
        let v = valor_de(&Fuente::BloqMayus, &l);
        assert_eq!(v.encendido, Some(true));
        assert_eq!(v.texto, None, "un interruptor no necesita texto");
        assert_eq!(valor_de(&Fuente::BloqNum, &l).encendido, Some(false));
    }

    #[test]
    fn una_app_abierta_se_reconoce_sin_distinguir_mayusculas_ni_ruta() {
        let l = con(Lecturas {
            procesos: vec!["teams.exe".into(), "excel.exe".into()],
            ..Default::default()
        });
        for escrito in [
            "teams.exe",
            "TEAMS.EXE",
            r"C:\Program Files\Teams\teams.exe",
        ] {
            let f = Fuente::AppAbierta {
                exe: escrito.into(),
            };
            assert_eq!(
                valor_de(&f, &l).encendido,
                Some(true),
                "no reconocio {escrito}"
            );
        }
        let f = Fuente::AppAbierta {
            exe: "word.exe".into(),
        };
        assert_eq!(valor_de(&f, &l).encendido, Some(false));
    }

    #[test]
    fn el_volumen_distingue_silenciado_de_bajado_del_todo() {
        // En Windows son dos cosas distintas, y la tecla tiene que decirlo: a
        // cero pero sin silenciar, la tecla sigue encendida.
        let a_cero = Lecturas {
            volumen: Some(0),
            altavoz_silenciado: Some(false),
            ..Default::default()
        };
        let v = valor_de(&Fuente::Volumen, &a_cero);
        assert_eq!(v.texto.unwrap(), "0%");
        assert_eq!(v.encendido, Some(true), "a cero no es silenciado");

        let silenciado_al_60 = Lecturas {
            volumen: Some(60),
            altavoz_silenciado: Some(true),
            ..Default::default()
        };
        let v = valor_de(&Fuente::Volumen, &silenciado_al_60);
        assert_eq!(
            v.texto.unwrap(),
            "60%",
            "el volumen sigue siendo 60 aunque calle"
        );
        assert_eq!(v.encendido, Some(false));
    }

    #[test]
    fn silenciado_apaga_la_tecla_y_no_al_reves() {
        // La cara 0 es el estado normal: con el micro abierto la tecla esta
        // encendida, y al cortarlo se apaga.
        let abierto = Lecturas {
            micro_silenciado: Some(false),
            ..Default::default()
        };
        assert_eq!(
            valor_de(&Fuente::MicroSilenciado, &abierto).encendido,
            Some(true)
        );

        let cortado = Lecturas {
            micro_silenciado: Some(true),
            ..Default::default()
        };
        assert_eq!(
            valor_de(&Fuente::MicroSilenciado, &cortado).encendido,
            Some(false)
        );
    }

    #[test]
    fn sin_microfono_la_tecla_no_se_inventa_nada() {
        let v = valor_de(&Fuente::MicroSilenciado, &Lecturas::default());
        assert_eq!(v, Valor::nada());
    }

    #[test]
    fn el_reloj_va_a_dos_cifras() {
        let l = con(Lecturas {
            hora: Some((9, 5)),
            ..Default::default()
        });
        assert_eq!(valor_de(&Fuente::Reloj, &l).texto.unwrap(), "09:05");
    }

    #[test]
    fn la_bateria_se_apaga_cuando_queda_poca() {
        // Para que una tecla de dos caras pueda cambiar sola al quedar poca.
        let baja = con(Lecturas {
            bateria: Some(15),
            ..Default::default()
        });
        let v = valor_de(&Fuente::Bateria, &baja);
        assert_eq!(v.texto.unwrap(), "15%");
        assert_eq!(v.encendido, Some(false));

        let llena = con(Lecturas {
            bateria: Some(80),
            ..Default::default()
        });
        assert_eq!(valor_de(&Fuente::Bateria, &llena).encendido, Some(true));
    }

    #[test]
    fn lo_que_no_se_puede_leer_no_dice_nada_en_vez_de_decir_cero() {
        // Un sobremesa no tiene bateria y una unidad de red caida no tiene
        // espacio libre. Un "0%" ahi seria una mentira, no un dato que falta.
        let vacias = Lecturas::default();
        assert_eq!(valor_de(&Fuente::Bateria, &vacias), Valor::nada());
        assert_eq!(valor_de(&Fuente::Reloj, &vacias), Valor::nada());
        assert_eq!(valor_de(&Fuente::MemoriaUsada, &vacias), Valor::nada());
        let f = Fuente::DiscoLibre {
            unidad: "Z:".into(),
        };
        assert_eq!(valor_de(&f, &vacias), Valor::nada());
    }

    #[test]
    fn la_unidad_se_escriba_como_se_escriba_es_la_misma() {
        for escrita in ["c", "C", "c:", "C:", "c:/", "C:\\"] {
            assert_eq!(normalizar_unidad(escrita), "C:", "fallo con {escrita}");
        }
    }

    #[test]
    fn el_disco_libre_cabe_en_una_esquina() {
        const GB: u64 = 1024 * 1024 * 1024;
        assert_eq!(tamano_corto(0), "0.0 GB");
        assert_eq!(tamano_corto(GB * 3 / 2), "1.5 GB");
        // A partir de diez se van los decimales: "128 GB" cabe donde no cabe
        // "128.4 GB", y medio giga no le importa a quien mira de reojo.
        assert_eq!(tamano_corto(GB * 128), "128 GB");
        assert_eq!(tamano_corto(GB * 2048), "2.0 TB");
    }

    #[test]
    fn una_fuente_de_una_version_mas_nueva_no_tumba_la_tecla() {
        // Sin la variante Desconocida, esto hacia que la tecla no se leyera, y
        // una tecla ilegible se lleva el deck entero: se da por corrupto, se
        // respalda y se arranca de cero. Pasaria en cuanto una version anadiera
        // una fuente nueva.
        #[derive(serde::Deserialize)]
        struct Prueba {
            #[serde(default, deserialize_with = "fuente_tolerante")]
            live: Option<Fuente>,
        }

        let p: Prueba =
            serde_json::from_str(r#"{ "live": { "type": "temperatura", "sonda": 2 } }"#)
                .expect("deberia leerse en vez de romper");
        let Some(Fuente::Desconocida { raw }) = &p.live else {
            panic!("deberia haberse envuelto");
        };
        assert_eq!(raw["type"], "temperatura");
        assert_eq!(raw["sonda"], 2);

        // Y no ensena nada, en vez de inventarse un valor.
        assert_eq!(
            valor_de(p.live.as_ref().unwrap(), &Lecturas::default()),
            Valor::nada()
        );
    }

    #[test]
    fn sin_fuente_no_hay_nada_que_envolver() {
        #[derive(serde::Deserialize)]
        struct Prueba {
            #[serde(default, deserialize_with = "fuente_tolerante")]
            live: Option<Fuente>,
        }
        let p: Prueba = serde_json::from_str("{}").unwrap();
        assert!(p.live.is_none());
        let p: Prueba = serde_json::from_str(r#"{ "live": null }"#).unwrap();
        assert!(p.live.is_none());
    }

    #[test]
    fn solo_se_pregunta_por_lo_que_alguna_tecla_pide() {
        // Un deck sin teclas vivas no puede costar ni una llamada a Windows.
        let l = leer(&[]);
        assert_eq!(l, Lecturas::default());
    }
}
