use crate::{
    activity,
    model::{Package, State},
};
use rune::{Image, Latest, Tx};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub static CHOOSE: rune::Icon = rune::Icon {
    name: "choose-package-icon",
    view: 24.0,
    body: include_str!("../assets/status/image-up.svg"),
};
const LIMIT: u64 = 1024 * 1024;
pub static RESET: rune::Icon = rune::Icon {
    name: "reset-package-icon",
    view: 24.0,
    body: include_str!("../assets/status/rotate-ccw.svg"),
};

pub fn identity(p: &Package) -> String {
    if let Some(e) = &p.extension {
        return activity::hash(("extension", e.id.to_ascii_lowercase()));
    }
    if let Some(e) = &p.watchlist {
        return activity::hash(("package", e.ecosystem.id(), &e.name));
    }
    if let Some(d) = &p.dependency {
        return activity::hash(("package", &d.ecosystem, &d.package));
    }
    if let Some(id) = &p.reference_id {
        return activity::hash(("catalog", id));
    }
    if p.detected {
        return activity::hash(("detected", p.group_key.as_deref().unwrap_or(&p.id)));
    }
    activity::hash(("catalog", &p.id))
}

fn file(directory: &Path, key: &str) -> Result<PathBuf, String> {
    if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid icon identity".into());
    }
    Ok(directory.join(format!("{key}.png")))
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    let input = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if !input.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Choose an image file".into());
    }
    let mut bytes = Vec::new();
    input
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    releasescout_engine::icons::normalize_icon(&bytes).map_err(|e| match e {
        releasescout_engine::Error::Source(message) => message,
        other => other.to_string(),
    })
}

fn import(directory: &Path, key: &str, source: &Path) -> Result<Vec<u8>, String> {
    let target = file(directory, key)?;
    let bytes = read(source)?;
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let mut saved = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
    saved.write_all(&bytes).map_err(|e| e.to_string())?;
    saved.as_file().sync_all().map_err(|e| e.to_string())?;
    saved.persist(target).map_err(|e| e.to_string())?;
    Ok(bytes)
}

enum Request {
    Load(Vec<String>),
    Import(String, PathBuf),
    Reset(String),
}
type IconBytes = Result<Option<Vec<u8>>, String>;
pub struct Reply {
    mutation: bool,
    entries: Vec<(String, IconBytes)>,
}
fn execute(directory: &Path, request: Request) -> Reply {
    match request {
        Request::Load(keys) => Reply {
            mutation: false,
            entries: keys
                .into_iter()
                .map(|key| {
                    let result =
                        file(directory, &key).and_then(|path| match std::fs::metadata(&path) {
                            Ok(_) => read(&path).map(Some),
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                            Err(e) => Err(e.to_string()),
                        });
                    (key, result)
                })
                .collect(),
        },
        Request::Import(key, source) => {
            let result = import(directory, &key, &source).map(Some);
            Reply {
                mutation: true,
                entries: vec![(key, result)],
            }
        }
        Request::Reset(key) => {
            let result = file(directory, &key).and_then(|path| match std::fs::remove_file(path) {
                Ok(()) => Ok(None),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.to_string()),
            });
            Reply {
                mutation: true,
                entries: vec![(key, result)],
            }
        }
    }
}

pub struct CustomIcons {
    pub enabled: bool,
    pub directory: PathBuf,
    pub work: Latest<Reply>,
    pub dialog: bool,
    pub error: Option<(String, String)>,
    pub picker: Option<crate::icon_picker::Picker>,
    pub(crate) loaded: HashMap<String, Option<Image>>,
}
impl Default for CustomIcons {
    fn default() -> Self {
        Self {
            enabled: false,
            directory: crate::live::cache_directory().with_file_name("custom-icons"),
            work: Latest::default(),
            dialog: false,
            error: None,
            picker: None,
            loaded: HashMap::new(),
        }
    }
}

impl State {
    fn icon_request(&mut self, request: Request) {
        let directory = self.custom_icons.directory.clone();
        self.custom_icons
            .work
            .request(request, move |request| execute(&directory, request));
    }

    pub fn choose_icon(&mut self, tx: &mut Tx) {
        if self.custom_icons.dialog || self.custom_icons.work.is_pending() {
            return;
        }
        let Some(key) = self
            .custom_icons
            .picker
            .as_ref()
            .map(|p| p.target.clone())
            .or_else(|| self.selected().map(identity))
        else {
            return;
        };
        self.custom_icons.picker = None;
        self.custom_icons.dialog = true;
        tx.open_file(
            rune::FileDialogOptions {
                title: "Choose icon".into(),
                ..Default::default()
            }
            .filter("Images", &["png", "jpg", "jpeg", "webp", "ico"]),
            move |s: &mut State, result, _| {
                s.custom_icons.dialog = false;
                match result {
                    Ok(Some(path)) => s.icon_request(Request::Import(key, path)),
                    Ok(None) => {}
                    Err(e) => s.custom_icons.error = Some((key, e.to_string())),
                }
            },
        );
    }

    pub fn reset_icon(&mut self) {
        if self.custom_icons.dialog || self.custom_icons.work.is_pending() {
            return;
        }
        if let Some(key) = self
            .custom_icons
            .picker
            .as_ref()
            .map(|p| p.target.clone())
            .or_else(|| self.selected().map(identity))
        {
            self.custom_icons.picker = None;
            self.icon_request(Request::Reset(key));
        }
    }

    pub fn poll_custom_icons(&mut self) -> bool {
        let mut changed = false;
        if let Some(reply) = self.custom_icons.work.take() {
            changed = true;
            for (key, result) in reply.entries {
                match result {
                    Ok(bytes) => {
                        let imported = bytes.is_some();
                        self.custom_icons
                            .loaded
                            .insert(key.clone(), bytes.map(|b| self.icons.load(b)));
                        if self
                            .custom_icons
                            .error
                            .as_ref()
                            .is_some_and(|(id, _)| id == &key)
                        {
                            self.custom_icons.error = None;
                        }
                        if reply.mutation {
                            if self.activity.data.icon_choices.remove(&key).is_some() {
                                self.activity.dirty = true;
                            }
                            self.status = if imported {
                                "Custom icon saved locally"
                            } else {
                                "Automatic icon restored"
                            }
                            .into();
                        }
                    }
                    Err(error) => {
                        if !reply.mutation {
                            self.custom_icons.loaded.insert(key.clone(), None);
                        }
                        self.custom_icons.error =
                            Some((key, format!("Custom icon not changed: {error}")));
                    }
                }
            }
        }
        let pool = if self
            .activity
            .data
            .icon_choices
            .values()
            .any(|v| matches!(v, crate::icon_picker::Choice::App(_)))
        {
            crate::icon_picker::app_pool(self)
        } else {
            HashMap::new()
        };
        for p in &mut self.catalog.packages {
            use crate::icon_picker::{Choice, symbol};
            let key = identity(p);
            let choice = self.activity.data.icon_choices.get(&key);
            let selected_symbol = match choice {
                Some(Choice::Symbol(id)) => symbol(id),
                _ => None,
            };
            let image = match choice {
                Some(Choice::App(source)) => pool.get(source).cloned(),
                Some(Choice::Symbol(_)) => None,
                None => self.custom_icons.loaded.get(&key).cloned().flatten(),
            };
            if p.custom_icon.as_ref().map(Image::id) != image.as_ref().map(Image::id)
                || p.custom_symbol.map(|i| i.name) != selected_symbol.map(|i| i.name)
            {
                p.custom_icon = image;
                p.custom_symbol = selected_symbol;
                changed = true;
            }
        }
        if self.custom_icons.enabled
            && !self.custom_icons.dialog
            && self.custom_icons.picker.is_none()
            && !self.custom_icons.work.is_pending()
        {
            let mut keys = Vec::new();
            for key in crate::icon_loader::priority_packages(self)
                .into_iter()
                .map(|i| identity(&self.catalog.packages[i]))
            {
                if !self.custom_icons.loaded.contains_key(&key) && !keys.contains(&key) {
                    keys.push(key);
                }
                if keys.len() == 16 {
                    break;
                }
            }
            if !keys.is_empty() {
                self.icon_request(Request::Load(keys));
                changed = true;
            }
        }
        changed
    }
}

pub fn scene(s: &mut State) {
    crate::grouped_scene(s);
    let key = identity(s.selected().unwrap());
    let image = s
        .icons
        .load(&include_bytes!("../assets/providers/winget.png")[..]);
    image.wait().unwrap();
    s.custom_icons.loaded.insert(key, Some(image));
    s.poll_custom_icons();
}

#[cfg(test)]
mod tests {
    use super::*;
    const PNG: &[u8] = include_bytes!("../assets/providers/winget.png");
    #[test]
    fn icon_import_is_copied_atomic_bounded_and_survives_source_removal() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("original.png");
        let saved = dir.path().join("saved");
        let key = activity::hash("entry");
        std::fs::write(&source, PNG).unwrap();
        import(&saved, &key, &source).unwrap();
        import(&saved, &key, &source).unwrap();
        std::fs::remove_file(&source).unwrap();
        assert!(read(&file(&saved, &key).unwrap()).is_ok());
        let before = std::fs::read(file(&saved, &key).unwrap()).unwrap();
        std::fs::write(&source, b"invalid image").unwrap();
        assert!(import(&saved, &key, &source).is_err());
        assert_eq!(std::fs::read(file(&saved, &key).unwrap()).unwrap(), before);
        std::fs::write(&source, vec![0; (LIMIT + 1) as usize]).unwrap();
        assert!(import(&saved, &key, &source).is_err());
        assert!(file(&saved, "../outside").is_err());
        let reply = execute(&saved, Request::Reset(key.clone()));
        assert!(reply.entries[0].1.is_ok());
        assert!(!file(&saved, &key).unwrap().exists());
    }
    #[test]
    fn stable_product_identity_is_not_a_display_name_or_observed_version() {
        let mut s = State::demo();
        crate::provider_badge::scene(&mut s);
        let mut p = s.selected().unwrap().clone();
        let key = identity(&p);
        p.id = "new observation".into();
        p.installed = Some("99.0".into());
        assert_eq!(identity(&p), key);
        p.group_key = Some("unrelated-product".into());
        assert_ne!(identity(&p), key);
        p.detected = false;
        p.reference_id = Some("gh".into());
        let reference = identity(&p);
        let fallback = s.catalog.packages.iter().find(|p| p.id == "gh").unwrap();
        assert_eq!(reference, identity(fallback));
        crate::package_ui::scene(&mut s);
        let declaration = identity(s.selected().unwrap());
        crate::watchlist_ui::scene(&mut s);
        let watch = s
            .catalog
            .packages
            .iter()
            .find(|p| p.watchlist.as_ref().is_some_and(|e| e.name == "next"))
            .unwrap();
        assert_eq!(declaration, identity(watch));
    }

    #[test]
    fn overrides_restore_from_disk_and_survive_observation_refreshes_without_replacing_automatic_icons()
     {
        let mut s = State::new();
        crate::grouped_scene(&mut s);
        let key = identity(s.selected().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("chosen.png");
        std::fs::write(&source, PNG).unwrap();
        import(dir.path(), &key, &source).unwrap();
        let mut reply = execute(dir.path(), Request::Load(vec![key]));
        let bytes = reply.entries.remove(0).1.unwrap().unwrap();
        let image = s.icons.load(bytes);
        image.wait().unwrap();
        let key = identity(s.selected().unwrap());
        s.custom_icons.loaded.insert(key, Some(image.clone()));
        s.poll_custom_icons();
        for p in s.selected_observations() {
            assert_eq!(p.custom_icon.as_ref().unwrap().id(), image.id());
        }
        s.rebuild_detected();
        s.poll_custom_icons();
        let observations = s.selected_observations();
        assert!(observations.len() > 1);
        for p in observations {
            assert_eq!(p.custom_icon.as_ref().unwrap().id(), image.id());
            assert!(p.icon.is_none());
        }
    }
}
