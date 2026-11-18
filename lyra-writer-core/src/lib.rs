//! Núcleo do Lyra Writer: as imagens oficiais do Lyra OS e os pendrives em
//! que elas podem ser gravadas.
//!
//! A lista de imagens ainda é fixa (a versão final lê um manifesto assinado
//! pela chave "Lyra OS Release"). A listagem de pendrives e a gravação de uma
//! ISO local são reais, pelo udisks2 ([`write_image`]).

use std::collections::HashMap;

mod write;
pub use write::{Phase, WriteError, write_image};

use serde::Serialize;
use zbus::blocking::Connection;
use zbus::blocking::fdo::ObjectManagerProxy;
use zbus::zvariant::{ObjectPath, OwnedValue};

/// Uma imagem que o Lyra Writer sabe gravar.
#[derive(Debug, Clone, Serialize)]
pub struct Image {
    pub id: &'static str,
    pub edition: &'static str,
    pub codename: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub recommended: bool,
}

/// As edições da versão atual. A versão final busca isto de um manifesto.
pub fn images() -> Vec<Image> {
    vec![
        Image {
            id: "desktop",
            edition: "Desktop",
            codename: "Odisseia",
            version: "16.1",
            description: "GNOME pronto para o dia a dia, em notebooks e computadores de mesa.",
            recommended: true,
        },
        Image {
            id: "server",
            edition: "Server",
            codename: "Delos",
            version: "16.1",
            description: "Sem interface gráfica, administrado pelo terminal ou pelo Vega Web.",
            recommended: false,
        },
    ]
}

/// Um pendrive em que se pode gravar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Device {
    /// Objeto do bloco no udisks2, usado depois para abrir o dispositivo.
    pub object: String,
    /// Caminho em /dev, só para mostrar.
    pub path: String,
    pub name: String,
    pub size: u64,
    /// Rótulo da imagem gravada nele (ISO 9660), se houver. É o que faz o
    /// Lyra Writer oferecer "Restaurar pendrive", como o Fedora Media Writer.
    pub live: Option<String>,
}

/// O que importa de um bloco e do drive dele para decidir se é um pendrive.
#[derive(Debug, Clone, Default)]
pub struct Candidate {
    pub object: String,
    pub size: u64,
    pub is_partition: bool,
    pub hint_system: bool,
    pub removable: bool,
    pub connection_bus: String,
    pub vendor: String,
    pub model: String,
    /// Drive do bloco no udisks2.
    pub drive: String,
    /// Sistema de arquivos e rótulo do bloco (IdType e IdLabel).
    pub id_type: String,
    pub id_label: String,
}

/// Só discos inteiros, removíveis, ligados por USB e com mídia. Discos
/// internos e partições nunca aparecem: é a primeira proteção contra apagar
/// o disco errado.
pub fn is_writable_target(c: &Candidate) -> bool {
    !c.is_partition && !c.hint_system && c.removable && c.connection_bus == "usb" && c.size > 0
}

fn to_device(c: &Candidate) -> Device {
    let name = [c.vendor.trim(), c.model.trim()]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    let path = c
        .object
        .rsplit('/')
        .next()
        .map(|leaf| format!("/dev/{leaf}"))
        .unwrap_or_default();
    Device {
        object: c.object.clone(),
        path,
        name: if name.is_empty() {
            "Pendrive".into()
        } else {
            name
        },
        size: c.size,
        live: None,
    }
}

/// Rótulo da imagem gravada num drive: o disco inteiro ou uma partição com
/// ISO 9660 (as ISOs híbridas aparecem dos dois jeitos).
fn live_label(candidates: &[Candidate], drive: &str) -> Option<String> {
    candidates
        .iter()
        .find(|c| c.drive == drive && c.id_type == "iso9660")
        .map(|c| {
            if c.id_label.is_empty() {
                "Imagem gravada".into()
            } else {
                c.id_label.clone()
            }
        })
}

/// Filtra e ordena os candidatos.
pub fn select(candidates: &[Candidate]) -> Vec<Device> {
    let mut devices: Vec<Device> = candidates
        .iter()
        .filter(|c| is_writable_target(c))
        .map(|c| Device {
            live: live_label(candidates, &c.drive),
            ..to_device(c)
        })
        .collect();
    devices.sort_by(|a, b| a.path.cmp(&b.path));
    devices
}

type Props = HashMap<String, OwnedValue>;

fn prop<'a, T>(props: &'a Props, key: &str) -> Option<T>
where
    T: TryFrom<&'a zbus::zvariant::Value<'a>>,
    zbus::zvariant::Error: From<<T as TryFrom<&'a zbus::zvariant::Value<'a>>>::Error>,
{
    props.get(key).and_then(|v| v.downcast_ref::<T>().ok())
}

fn text(props: &Props, key: &str) -> String {
    prop::<&str>(props, key).unwrap_or_default().to_string()
}

/// Pendrives presentes agora, pelo udisks2 no barramento do sistema.
pub fn devices() -> zbus::Result<Vec<Device>> {
    let conn = Connection::system()?;
    let manager = ObjectManagerProxy::builder(&conn)
        .destination("org.freedesktop.UDisks2")?
        .path("/org/freedesktop/UDisks2")?
        .build()?;
    let objects = manager.get_managed_objects()?;

    let mut drives: HashMap<String, Props> = HashMap::new();
    for (path, ifaces) in &objects {
        if let Some(drive) = ifaces.get("org.freedesktop.UDisks2.Drive") {
            drives.insert(path.to_string(), drive.clone());
        }
    }

    let mut candidates = Vec::new();
    for (path, ifaces) in &objects {
        let Some(block) = ifaces.get("org.freedesktop.UDisks2.Block") else {
            continue;
        };
        let drive_path = prop::<ObjectPath>(block, "Drive")
            .map(|p| p.to_string())
            .unwrap_or_default();
        let Some(drive) = drives.get(&drive_path) else {
            continue;
        };
        candidates.push(Candidate {
            object: path.to_string(),
            size: prop::<u64>(block, "Size").unwrap_or(0),
            is_partition: ifaces.contains_key("org.freedesktop.UDisks2.Partition"),
            hint_system: prop::<bool>(block, "HintSystem").unwrap_or(true),
            removable: prop::<bool>(drive, "Removable").unwrap_or(false)
                || prop::<bool>(drive, "MediaRemovable").unwrap_or(false),
            connection_bus: text(drive, "ConnectionBus"),
            vendor: text(drive, "Vendor"),
            model: text(drive, "Model"),
            drive: drive_path.clone(),
            id_type: text(block, "IdType"),
            id_label: text(block, "IdLabel"),
        });
    }
    Ok(select(&candidates))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usb() -> Candidate {
        Candidate {
            object: "/org/freedesktop/UDisks2/block_devices/sdb".into(),
            size: 32_000_000_000,
            removable: true,
            connection_bus: "usb".into(),
            vendor: "SanDisk".into(),
            model: "Ultra".into(),
            ..Default::default()
        }
    }

    #[test]
    fn usb_stick_is_listed() {
        let list = select(&[usb()]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].path, "/dev/sdb");
        assert_eq!(list[0].name, "SanDisk Ultra");
    }

    #[test]
    fn internal_disk_is_never_listed() {
        let internal = Candidate {
            removable: false,
            connection_bus: String::new(),
            hint_system: true,
            ..usb()
        };
        assert!(select(&[internal]).is_empty());
    }

    #[test]
    fn partitions_are_never_listed() {
        let part = Candidate {
            is_partition: true,
            ..usb()
        };
        assert!(select(&[part]).is_empty());
    }

    #[test]
    fn system_hint_wins_over_removable() {
        let sys = Candidate {
            hint_system: true,
            ..usb()
        };
        assert!(select(&[sys]).is_empty());
    }

    #[test]
    fn written_stick_offers_restore() {
        let disk = Candidate {
            drive: "d1".into(),
            ..usb()
        };
        let part = Candidate {
            object: "/org/freedesktop/UDisks2/block_devices/sdb1".into(),
            is_partition: true,
            drive: "d1".into(),
            id_type: "iso9660".into(),
            id_label: "Lyra-OS-16.1".into(),
            ..usb()
        };
        let list = select(&[disk, part]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].live.as_deref(), Some("Lyra-OS-16.1"));
    }

    #[test]
    fn plain_stick_has_nothing_to_restore() {
        let disk = Candidate {
            drive: "d1".into(),
            id_type: "vfat".into(),
            ..usb()
        };
        assert_eq!(select(&[disk])[0].live, None);
    }

    #[test]
    fn empty_card_reader_is_skipped() {
        let empty = Candidate { size: 0, ..usb() };
        assert!(select(&[empty]).is_empty());
    }
}
