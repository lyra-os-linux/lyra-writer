//! Gravação de uma ISO num pendrive pelo udisks2.
//!
//! O app não roda como root: o bloco é aberto pelo `Block.OpenDevice` do
//! udisks2, que pede a senha pelo polkit (`org.freedesktop.udisks2.open-device`)
//! e devolve um descritor já aberto para escrita, com `O_EXCL`. Antes disso as
//! partições montadas do pendrive são desmontadas, também pelo udisks2.

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use zbus::blocking::fdo::ObjectManagerProxy;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, Value};

const UDISKS: &str = "org.freedesktop.UDisks2";
const BLOCK: &str = "org.freedesktop.UDisks2.Block";
const PARTITION: &str = "org.freedesktop.UDisks2.Partition";
const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";

/// Tamanho de cada bloco copiado. 4 MiB dá progresso fluido sem custo de
/// chamadas demais.
const CHUNK: usize = 4 * 1024 * 1024;

/// Por que a gravação não terminou.
#[derive(Debug)]
pub enum WriteError {
    /// O dispositivo não passa no filtro de pendrives (disco interno,
    /// partição ou não está mais conectado).
    NotAllowed,
    /// A ISO é maior que o pendrive.
    TooSmall { image: u64, device: u64 },
    /// A senha não foi dada ou o polkit negou.
    NotAuthorized,
    /// Uma partição continua em uso e não pôde ser desmontada.
    Busy(String),
    /// O usuário cancelou.
    Cancelled,
    /// Falha ao ler a ISO.
    Image(io::Error),
    /// Falha ao escrever no pendrive (desconectado, mídia com defeito…).
    Device(io::Error),
    /// Erro do udisks2 ou do barramento.
    Udisks(zbus::Error),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAllowed => {
                f.write_str("Este dispositivo não é um pendrive que o Lyra Writer possa gravar.")
            }
            Self::TooSmall { image, device } => write!(
                f,
                "A imagem ({:.1} GB) não cabe no pendrive ({:.1} GB).",
                *image as f64 / 1e9,
                *device as f64 / 1e9
            ),
            Self::NotAuthorized => f.write_str("A gravação não foi autorizada."),
            Self::Busy(e) => write!(f, "O pendrive está em uso e não pôde ser desmontado: {e}"),
            Self::Cancelled => f.write_str("A gravação foi cancelada."),
            Self::Image(e) => write!(f, "Não foi possível ler a imagem: {e}"),
            Self::Device(e) if e.kind() == io::ErrorKind::StorageFull => {
                f.write_str("O pendrive foi desconectado ou é menor do que informa.")
            }
            Self::Device(e) => write!(f, "Não foi possível escrever no pendrive: {e}"),
            Self::Udisks(e) => write!(f, "Erro do udisks2: {e}"),
        }
    }
}

impl std::error::Error for WriteError {}

/// Erros do udisks2 que significam "o polkit não autorizou".
fn is_auth_error(e: &zbus::Error) -> bool {
    matches!(e, zbus::Error::MethodError(name, _, _) if name.as_str().starts_with("org.freedesktop.UDisks2.Error.NotAuthorized"))
}

fn udisks_error(e: zbus::Error) -> WriteError {
    if is_auth_error(&e) {
        WriteError::NotAuthorized
    } else {
        WriteError::Udisks(e)
    }
}

/// Grava `image` no pendrive `object` (caminho do bloco no udisks2).
///
/// O dispositivo é conferido de novo aqui, e não só na lista da interface:
/// só discos que passam em [`crate::is_writable_target`] são abertos.
/// `progress` recebe os bytes gravados e o total; `cancel` interrompe a
/// cópia entre um bloco e outro.
pub fn write_image(
    image: &Path,
    object: &str,
    progress: impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), WriteError> {
    let device = crate::devices()
        .map_err(udisks_error)?
        .into_iter()
        .find(|d| d.object == object)
        .ok_or(WriteError::NotAllowed)?;
    let conn = Connection::system().map_err(udisks_error)?;
    write_block(&conn, image, object, device.size, progress, cancel)
}

/// A gravação em si, sem o filtro de pendrives. Separada para o teste com
/// dispositivo loop, que o filtro recusa por não ser USB.
pub(crate) fn write_block(
    conn: &Connection,
    image: &Path,
    object: &str,
    device_size: u64,
    mut progress: impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), WriteError> {
    let mut source = File::open(image).map_err(WriteError::Image)?;
    let total = source.metadata().map_err(WriteError::Image)?.len();
    if total > device_size {
        return Err(WriteError::TooSmall {
            image: total,
            device: device_size,
        });
    }

    unmount_all(conn, object)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(WriteError::Cancelled);
    }
    let mut target = File::from(open_device(conn, object)?);

    let mut buf = vec![0u8; CHUNK];
    let mut written = 0u64;
    progress(0, total);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(WriteError::Cancelled);
        }
        let n = read_full(&mut source, &mut buf).map_err(WriteError::Image)?;
        if n == 0 {
            break;
        }
        target.write_all(&buf[..n]).map_err(WriteError::Device)?;
        written += n as u64;
        progress(written, total);
    }
    target.sync_all().map_err(WriteError::Device)
}

/// Lê até encher o buffer ou acabar o arquivo.
fn read_full(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

fn proxy<'a>(conn: &Connection, path: &'a str, iface: &'a str) -> Result<Proxy<'a>, WriteError> {
    Proxy::new(conn, UDISKS, path, iface).map_err(udisks_error)
}

/// Abre o bloco para escrita pelo udisks2. É aqui que o polkit pede a senha.
fn open_device(conn: &Connection, object: &str) -> Result<OwnedFd, WriteError> {
    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("flags", Value::from(libc::O_EXCL | libc::O_CLOEXEC));
    let fd: zbus::zvariant::OwnedFd = proxy(conn, object, BLOCK)?
        .call("OpenDevice", &("rw", options))
        .map_err(udisks_error)?;
    Ok(fd.into())
}

/// Desmonta tudo o que estiver montado no disco: o próprio bloco e as
/// partições dele.
fn unmount_all(conn: &Connection, object: &str) -> Result<(), WriteError> {
    let manager = ObjectManagerProxy::builder(conn)
        .destination(UDISKS)
        .and_then(|b| b.path("/org/freedesktop/UDisks2"))
        .and_then(|b| b.build())
        .map_err(udisks_error)?;
    let objects = manager
        .get_managed_objects()
        .map_err(|e| udisks_error(e.into()))?;

    for (path, ifaces) in &objects {
        let Some(fs) = ifaces.get(FILESYSTEM) else {
            continue;
        };
        let on_disk = path.as_str() == object
            || ifaces
                .get(PARTITION)
                .and_then(|p| p.get("Table"))
                .and_then(|t| t.downcast_ref::<ObjectPath>().ok())
                .is_some_and(|t| t.as_str() == object);
        if !on_disk {
            continue;
        }
        let mounted = fs
            .get("MountPoints")
            .and_then(|m| m.downcast_ref::<zbus::zvariant::Array>().ok())
            .is_some_and(|a| !a.is_empty());
        if mounted {
            unmount(conn, path)?;
        }
    }
    Ok(())
}

fn unmount(conn: &Connection, path: &OwnedObjectPath) -> Result<(), WriteError> {
    let options: HashMap<&str, Value> = HashMap::new();
    proxy(conn, path.as_str(), FILESYSTEM)?
        .call::<_, _, ()>("Unmount", &(options,))
        .map_err(|e| match e {
            e if is_auth_error(&e) => WriteError::NotAuthorized,
            zbus::Error::MethodError(_, Some(msg), _) => WriteError::Busy(msg),
            e => WriteError::Udisks(e),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_full_fills_across_short_reads() {
        // Um leitor que entrega 3 bytes por vez.
        struct Slow<'a>(&'a [u8]);
        impl Read for Slow<'_> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                let n = self.0.len().min(buf.len()).min(3);
                buf[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }
        let data: Vec<u8> = (0..10).collect();
        let mut buf = [0u8; 8];
        let mut r = Slow(&data);
        assert_eq!(read_full(&mut r, &mut buf).unwrap(), 8);
        assert_eq!(&buf, &data[..8]);
        assert_eq!(read_full(&mut r, &mut buf).unwrap(), 2);
        assert_eq!(read_full(&mut r, &mut buf).unwrap(), 0);
    }

    #[test]
    fn auth_errors_are_recognized() {
        let msg = zbus::message::Message::method_call("/", "Ping")
            .unwrap()
            .build(&())
            .unwrap();
        let err =
            |name: &str| zbus::Error::MethodError(name.try_into().unwrap(), None, msg.clone());
        assert!(matches!(
            udisks_error(err("org.freedesktop.UDisks2.Error.NotAuthorizedDismissed")),
            WriteError::NotAuthorized
        ));
        assert!(matches!(
            udisks_error(err("org.freedesktop.UDisks2.Error.NotAuthorizedCanObtain")),
            WriteError::NotAuthorized
        ));
        assert!(matches!(
            udisks_error(err("org.freedesktop.UDisks2.Error.Failed")),
            WriteError::Udisks(_)
        ));
    }

    #[test]
    fn image_larger_than_device_is_refused() {
        let Ok(conn) = Connection::system() else {
            return; // sem barramento do sistema (CI mínima)
        };
        let dir = std::env::temp_dir().join(format!("lyra-writer-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let iso = dir.join("big.iso");
        std::fs::write(&iso, vec![0u8; 2048]).unwrap();
        let r = write_block(
            &conn,
            &iso,
            "/nao/usado",
            1024,
            |_, _| {},
            &AtomicBool::new(false),
        );
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(matches!(
            r,
            Err(WriteError::TooSmall {
                image: 2048,
                device: 1024
            })
        ));
    }

    /// Gravação de verdade num dispositivo loop criado pelo udisks2.
    /// Roda só à mão:
    ///
    /// ```text
    /// truncate -s 4G /tmp/disco.img
    /// udisksctl loop-setup -f /tmp/disco.img      # → /dev/loopN
    /// # sem isto o loop some ao desmontar a partição gravada
    /// busctl call org.freedesktop.UDisks2 /org/freedesktop/UDisks2/block_devices/loopN \
    ///   org.freedesktop.UDisks2.Loop SetAutoclear 'ba{sv}' false 0
    /// LYRA_WRITER_TEST_BLOCK=/org/freedesktop/UDisks2/block_devices/loopN \
    /// LYRA_WRITER_TEST_ISO=~/Downloads/LyraOS-Desktop-16.1-x86_64.iso \
    /// LYRA_WRITER_TEST_BACKING=/tmp/disco.img \
    ///   cargo test -p lyra-writer-core -- --ignored --nocapture loop_write
    /// ```
    #[test]
    #[ignore = "precisa de um dispositivo loop e da senha no polkit"]
    fn loop_write() {
        let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("defina {k}"));
        let (block, iso, backing) = (
            var("LYRA_WRITER_TEST_BLOCK"),
            var("LYRA_WRITER_TEST_ISO"),
            var("LYRA_WRITER_TEST_BACKING"),
        );
        let conn = Connection::system().unwrap();
        let size = std::fs::metadata(&backing).unwrap().len();
        let mut last = 0;
        write_block(
            &conn,
            Path::new(&iso),
            &block,
            size,
            |w, t| {
                let p = w * 100 / t.max(1);
                if p != last {
                    eprint!("\r{p}%");
                    last = p;
                }
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        eprintln!();

        // O começo do disco tem de ser idêntico à ISO.
        let (mut a, mut b) = (File::open(&iso).unwrap(), File::open(&backing).unwrap());
        let (mut x, mut y) = (vec![0u8; CHUNK], vec![0u8; CHUNK]);
        loop {
            let n = read_full(&mut a, &mut x).unwrap();
            if n == 0 {
                break;
            }
            assert_eq!(read_full(&mut b, &mut y[..n]).unwrap(), n);
            assert!(x[..n] == y[..n], "conteúdo diferente da ISO");
        }
    }
}
