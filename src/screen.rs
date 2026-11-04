//! Capture d'une partie de la fenêtre du jeu, telle qu'elle est à l'écran (X11 sous Linux, y
//! compris XWayland ; GDI sous Windows). Rien n'est capturé si la fenêtre du jeu est introuvable.

use image::RgbImage;

/// Titre de la fenêtre du jeu (Steam, Proton, version autonome).
const TITLE: &str = "Path of Exile 2";

/// Zone de la fenêtre du jeu, en pixels de son intérieur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

pub trait Grabber {
    /// Taille de l'intérieur de la fenêtre du jeu (largeur, hauteur), si elle est ouverte.
    fn game_size(&mut self) -> Option<(u32, u32)>;
    /// Pixels d'une zone de la fenêtre du jeu (repère de son intérieur).
    fn grab(&mut self, r: Rect) -> Option<RgbImage>;
}

pub fn grabber() -> Option<Box<dyn Grabber + Send>> {
    imp::open().map(|g| Box::new(g) as Box<dyn Grabber + Send>)
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{Rect, TITLE};
    use image::RgbImage;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, ImageFormat, Window};
    use x11rb::rust_connection::RustConnection;

    pub struct X11 {
        conn: RustConnection,
        root: Window,
        net_client_list: u32,
        net_wm_name: u32,
        utf8: u32,
        game: Option<Window>,
    }

    pub fn open() -> Option<X11> {
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen)?.root;
        let atom = |name: &str| Some(conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok()?.atom);
        let (net_client_list, net_wm_name, utf8) = (atom("_NET_CLIENT_LIST")?, atom("_NET_WM_NAME")?, atom("UTF8_STRING")?);
        Some(X11 { conn, root, net_client_list, net_wm_name, utf8, game: None })
    }

    impl X11 {
        fn title(&self, w: Window) -> Option<String> {
            let utf = self.conn.get_property(false, w, self.net_wm_name, self.utf8, 0, 256).ok()?.reply().ok()?;
            if !utf.value.is_empty() {
                return Some(String::from_utf8_lossy(&utf.value).into_owned());
            }
            let plain = self.conn.get_property(false, w, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 256).ok()?.reply().ok()?;
            Some(String::from_utf8_lossy(&plain.value).into_owned())
        }

        fn find(&self) -> Option<Window> {
            let list = self.conn.get_property(false, self.root, self.net_client_list, AtomEnum::WINDOW, 0, 4096).ok()?.reply().ok()?;
            let found = list.value32()?.find(|&w| self.title(w).is_some_and(|t| t == TITLE));
            found
        }

        /// Fenêtre du jeu (retrouvée si elle a changé) : position à l'écran et taille.
        fn place(&mut self) -> Option<(i32, i32, u32, u32)> {
            for _ in 0..2 {
                let w = match self.game {
                    Some(w) => w,
                    None => self.find()?,
                };
                let geo = self.conn.get_geometry(w).ok().and_then(|c| c.reply().ok());
                let pos = self.conn.translate_coordinates(w, self.root, 0, 0).ok().and_then(|c| c.reply().ok());
                if let (Some(g), Some(p)) = (geo, pos) {
                    self.game = Some(w);
                    return Some((p.dst_x as i32, p.dst_y as i32, g.width as u32, g.height as u32));
                }
                self.game = None;
            }
            None
        }
    }

    impl super::Grabber for X11 {
        fn game_size(&mut self) -> Option<(u32, u32)> {
            self.place().map(|(_, _, w, h)| (w, h))
        }

        fn grab(&mut self, r: Rect) -> Option<RgbImage> {
            let (gx, gy, _, _) = self.place()?;
            // Ce qui est à l'écran à cet endroit (fenêtre racine) : marche aussi pour une fenêtre
            // redirigée par un compositeur.
            let img = self.conn.get_image(ImageFormat::Z_PIXMAP, self.root, (gx + r.x) as i16, (gy + r.y) as i16, r.w as u16, r.h as u16, !0).ok()?.reply().ok()?;
            let px = r.w as usize * r.h as usize;
            if img.data.len() < px * 4 {
                return None;
            }
            // 32 bits par pixel, BGRX.
            let mut out = RgbImage::new(r.w, r.h);
            for (o, p) in out.pixels_mut().zip(img.data.chunks_exact(4)) {
                o.0 = [p[2], p[1], p[0]];
            }
            Some(out)
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::{Rect, TITLE};
    use image::RgbImage;
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{BitBlt, ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY};
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetClientRect, IsWindow};

    pub struct Gdi {
        game: HWND,
    }

    // Une poignée de fenêtre n'est qu'un numéro : elle peut changer de fil.
    unsafe impl Send for Gdi {}

    pub fn open() -> Option<Gdi> {
        Some(Gdi { game: std::ptr::null_mut() })
    }

    impl Gdi {
        /// Intérieur de la fenêtre du jeu, à l'écran.
        fn place(&mut self) -> Option<(i32, i32, u32, u32)> {
            unsafe {
                if self.game.is_null() || IsWindow(self.game) == 0 {
                    let title: Vec<u16> = TITLE.encode_utf16().chain([0]).collect();
                    self.game = FindWindowW(std::ptr::null(), title.as_ptr());
                }
                if self.game.is_null() {
                    return None;
                }
                let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                let mut p = POINT { x: 0, y: 0 };
                if GetClientRect(self.game, &mut rc) == 0 || ClientToScreen(self.game, &mut p) == 0 {
                    self.game = std::ptr::null_mut();
                    return None;
                }
                Some((p.x, p.y, (rc.right - rc.left).max(0) as u32, (rc.bottom - rc.top).max(0) as u32))
            }
        }
    }

    impl super::Grabber for Gdi {
        fn game_size(&mut self) -> Option<(u32, u32)> {
            self.place().map(|(_, _, w, h)| (w, h))
        }

        fn grab(&mut self, r: Rect) -> Option<RgbImage> {
            let (gx, gy, _, _) = self.place()?;
            unsafe {
                // Ce qui est à l'écran (le jeu en plein écran fenêtré) plutôt que le contenu de
                // sa fenêtre, souvent noir pour un rendu DirectX.
                let screen = GetDC(std::ptr::null_mut());
                let mem = CreateCompatibleDC(screen);
                let bmp = CreateCompatibleBitmap(screen, r.w as i32, r.h as i32);
                let old = SelectObject(mem, bmp);
                let ok = BitBlt(mem, 0, 0, r.w as i32, r.h as i32, screen, gx + r.x, gy + r.y, SRCCOPY) != 0;
                let mut info: BITMAPINFO = std::mem::zeroed();
                info.bmiHeader = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: r.w as i32, biHeight: -(r.h as i32), biPlanes: 1, biBitCount: 32, biCompression: BI_RGB, ..std::mem::zeroed() };
                let mut data = vec![0u8; r.w as usize * r.h as usize * 4];
                let lines = if ok { GetDIBits(mem, bmp, 0, r.h, data.as_mut_ptr().cast(), &mut info, DIB_RGB_COLORS) } else { 0 };
                SelectObject(mem, old);
                DeleteObject(bmp);
                DeleteDC(mem);
                ReleaseDC(std::ptr::null_mut(), screen);
                if lines as u32 != r.h {
                    return None;
                }
                let mut out = RgbImage::new(r.w, r.h);
                for (o, p) in out.pixels_mut().zip(data.chunks_exact(4)) {
                    o.0 = [p[2], p[1], p[0]];
                }
                Some(out)
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    pub struct None_;
    impl super::Grabber for None_ {
        fn game_size(&mut self) -> Option<(u32, u32)> {
            None
        }
        fn grab(&mut self, _: super::Rect) -> Option<image::RgbImage> {
            None
        }
    }
    pub fn open() -> Option<None_> {
        None
    }
}
