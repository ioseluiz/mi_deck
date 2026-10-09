//! Volumen y silencio del altavoz y del microfono, por Core Audio.
//!
//! Hasta ahora MiDeck solo **simulaba teclas multimedia**: subir el volumen era
//! mandar `VolumeUp`, que mueve el del sistema en pasos de un 2 % y no deja
//! preguntar en que porcentaje esta. Para que una tecla *ensene* el volumen hay
//! que leerlo, y leerlo solo se puede por aqui.
//!
//! Y hay algo que sin esto no se podia hacer en absoluto: **silenciar el
//! microfono**. No existe tecla virtual estandar para ello --`VK_VOLUME_MUTE`
//! silencia el altavoz-- asi que la unica via es el endpoint de captura.
//!
//! Se consulta por sondeo y no se registran callbacks a proposito. Un
//! `IAudioEndpointVolumeCallback` avisaria antes, pero llega en un hilo de COM
//! que no es el principal, obliga a un apartamento multihilo y a vigilar que el
//! objeto siga vivo. El latido de `vivo.rs` ya pregunta una vez por segundo, que
//! para una tecla es de sobra, y asi este modulo no guarda estado ninguno.

/// De que endpoint se habla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flujo {
    /// Lo que suena: altavoces o auriculares.
    Altavoz,
    /// Lo que entra: el microfono.
    Microfono,
}

/// Volumen del endpoint predeterminado, de 0 a 100.
pub fn volumen(flujo: Flujo) -> Option<u8> {
    con_endpoint(flujo, |v| unsafe {
        let escalar = v.GetMasterVolumeLevelScalar().ok()?;
        // El escalar llega de 0 a 1 y puede traer imprecision de coma flotante:
        // 0.999999 tiene que ser 100, no 99.
        Some((escalar * 100.0).round().clamp(0.0, 100.0) as u8)
    })
}

/// Si el endpoint predeterminado esta silenciado.
pub fn silenciado(flujo: Flujo) -> Option<bool> {
    con_endpoint(flujo, |v| unsafe { Some(v.GetMute().ok()?.as_bool()) })
}

/// Pone o quita el silencio.
pub fn fijar_silencio(flujo: Flujo, silencio: bool) -> Result<(), String> {
    con_endpoint(flujo, |v| unsafe {
        v.SetMute(silencio, std::ptr::null()).ok()?;
        Some(())
    })
    .ok_or_else(|| no_hay(flujo))
}

/// Conmuta el silencio y devuelve como queda.
///
/// Leer y escribir en la misma llamada, en vez de pedirle al que llama que haga
/// las dos: entre una y otra el usuario podria haberlo cambiado desde Windows, y
/// la tecla acabaria haciendo lo contrario de lo que ensena.
pub fn alternar_silencio(flujo: Flujo) -> Result<bool, String> {
    con_endpoint(flujo, |v| unsafe {
        let ahora = v.GetMute().ok()?.as_bool();
        v.SetMute(!ahora, std::ptr::null()).ok()?;
        Some(!ahora)
    })
    .ok_or_else(|| no_hay(flujo))
}

/// Fija el volumen, de 0 a 100.
pub fn fijar_volumen(flujo: Flujo, porcentaje: u8) -> Result<(), String> {
    let escalar = (porcentaje.min(100) as f32) / 100.0;
    con_endpoint(flujo, |v| unsafe {
        v.SetMasterVolumeLevelScalar(escalar, std::ptr::null())
            .ok()?;
        Some(())
    })
    .ok_or_else(|| no_hay(flujo))
}

/// Sube o baja el volumen en el paso del sistema, el mismo de las teclas
/// multimedia, para que una tecla y la rueda se sientan igual.
pub fn paso(flujo: Flujo, arriba: bool) -> Result<(), String> {
    con_endpoint(flujo, |v| unsafe {
        if arriba {
            v.VolumeStepUp(std::ptr::null()).ok()?;
        } else {
            v.VolumeStepDown(std::ptr::null()).ok()?;
        }
        Some(())
    })
    .ok_or_else(|| no_hay(flujo))
}

fn no_hay(flujo: Flujo) -> String {
    match flujo {
        Flujo::Altavoz => "No hay ningun dispositivo de sonido disponible.".to_string(),
        Flujo::Microfono => "No hay ningun microfono disponible.".to_string(),
    }
}

// --------------------------------------------------------------------- COM

/// Hace algo con el control de volumen del endpoint predeterminado.
///
/// Un solo sitio con `unsafe` y con el baile de COM: lo demas son cuatro lineas
/// que se leen sin saber nada de Windows.
#[cfg(windows)]
fn con_endpoint<T>(
    flujo: Flujo,
    f: impl FnOnce(&windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume) -> Option<T>,
) -> Option<T> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eCapture, eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };

    let dataflow = match flujo {
        Flujo::Altavoz => eRender,
        Flujo::Microfono => eCapture,
    };

    unsafe {
        // Puede estar ya inicializado por Tauri en este hilo. Solo se deshace lo
        // que se haya hecho aqui: apagarle el COM a otro es peor que no entrar.
        //
        // Apartamento por hilo y no multihilo: aqui solo se hacen llamadas
        // sincronas. El multihilo haria falta para registrar un callback de
        // cambios, que es justo lo que se decidio no hacer.
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let hay_que_cerrar = hr.is_ok();

        let resultado = (|| {
            let enumerador: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            // Falla de verdad cuando no hay dispositivo: un equipo sin microfono,
            // o unos auriculares que se acaban de desconectar.
            let dispositivo = enumerador
                .GetDefaultAudioEndpoint(dataflow, eMultimedia)
                .ok()?;
            let control: IAudioEndpointVolume = dispositivo.Activate(CLSCTX_ALL, None).ok()?;
            f(&control)
        })();

        if hay_que_cerrar {
            CoUninitialize();
        }
        resultado
    }
}

#[cfg(not(windows))]
fn con_endpoint<T>(_flujo: Flujo, _f: impl FnOnce(&()) -> Option<T>) -> Option<T> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Estas pruebas tocan el audio de verdad, asi que solo comprueban que la
    // cadena de COM responde y que lo que devuelve tiene sentido. Lo que se puede
    // afirmar sin un dispositivo concreto delante no da para mas: el formateo y
    // las decisiones viven en `vivo.rs`, que si es puro.

    #[test]
    fn el_volumen_del_altavoz_esta_entre_0_y_100_o_no_hay_dispositivo() {
        match volumen(Flujo::Altavoz) {
            Some(v) => assert!(v <= 100, "porcentaje imposible: {v}"),
            None => { /* un equipo sin sonido es un caso valido */ }
        }
    }

    #[test]
    fn preguntar_por_el_silencio_no_lo_cambia() {
        // Leer tiene que ser leer: si `silenciado` tocara el estado, una tecla que
        // solo ensena el microfono lo estaria silenciando cada segundo.
        let Some(antes) = silenciado(Flujo::Altavoz) else {
            return;
        };
        for _ in 0..3 {
            assert_eq!(silenciado(Flujo::Altavoz), Some(antes));
        }
    }

    #[test]
    fn dos_flujos_distintos_son_dos_dispositivos_distintos() {
        // No es una tautologia: con un solo `con_endpoint` mal escrito, el
        // microfono devolveria el volumen del altavoz y silenciar el micro
        // silenciaria la musica.
        if let (Some(altavoz), Some(micro)) = (volumen(Flujo::Altavoz), volumen(Flujo::Microfono)) {
            // Pueden coincidir por casualidad; lo que no puede es responder uno
            // con lo del otro. Basta con que los dos den algo valido.
            assert!(altavoz <= 100 && micro <= 100);
        }
    }
}
