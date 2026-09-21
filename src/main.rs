// Terminal Tetris for PS3 Linux (ppc64), no external crates.
// Build: cargo build --release && ./target/release/tetris
// Keys: arrows or a/d=move, w/space=rotate, s=soft drop, c=hold, p=pause, q=quit.
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const W: i8 = 10;
const H: i8 = 20;

// Piece rotations as (x,y) cells.
const PIECES: [&[[(i8, i8); 4]]; 7] = [
    &[[(0,0),(1,0),(2,0),(3,0)], [(2,0),(2,1),(2,2),(2,3)], [(0,2),(1,2),(2,2),(3,2)], [(0,0),(0,1),(0,2),(0,3)]], // I
    &[[(0,0),(1,0),(0,1),(1,1)]], // O
    &[[(1,0),(0,1),(1,1),(2,1)], [(1,0),(1,1),(1,2),(2,1)], [(0,1),(1,1),(2,1),(1,2)], [(1,0),(0,1),(1,1),(1,2)]], // T
    &[[(0,0),(0,1),(1,1),(2,1)], [(1,0),(2,0),(1,1),(1,2)], [(0,1),(1,1),(2,1),(2,2)], [(1,0),(1,1),(0,2),(1,2)]], // J
    &[[(2,0),(0,1),(1,1),(2,1)], [(1,0),(1,1),(1,2),(2,2)], [(0,1),(1,1),(2,1),(0,2)], [(0,0),(1,0),(1,1),(1,2)]], // L
    &[[(1,0),(2,0),(0,1),(1,1)], [(1,0),(1,1),(2,1),(2,2)]], // S
    &[[(0,0),(1,0),(1,1),(2,1)], [(2,0),(1,1),(2,1),(1,2)]], // Z
];
const COLORS: [&str; 7] = ["\x1b[96m", "\x1b[93m", "\x1b[95m", "\x1b[94m", "\x1b[33m", "\x1b[92m", "\x1b[91m"];

extern "C" {
    fn tcgetattr(fd: i32, t: *mut u8) -> i32;
    fn tcsetattr(fd: i32, act: i32, t: *const u8) -> i32;
    fn poll(f: *mut PollFd, n: u64, ms: i32) -> i32;
    fn read(fd: i32, b: *mut u8, n: usize) -> isize;
}
#[repr(C)]
struct PollFd { fd: i32, events: i16, revents: i16 }

// ppc Linux termios: lflag u32 @12 (big-endian), cc @17. ISIG=0x80 ICANON=0x100 ECHO=0x8.
struct TermRaw { orig: [u8; 64] }
impl TermRaw {
    fn new() -> TermRaw {
        let mut t = [0u8; 64];
        unsafe {
            tcgetattr(0, t.as_mut_ptr());
            let orig = t;
            let l = u32::from_be_bytes(t[12..16].try_into().unwrap()) & !0x188;
            t[12..16].copy_from_slice(&l.to_be_bytes());
            t[22] = 1; // VMIN
            t[24] = 0; // VTIME
            tcsetattr(0, 0, t.as_ptr());
            TermRaw { orig }
        }
    }
}
impl Drop for TermRaw {
    fn drop(&mut self) { unsafe { tcsetattr(0, 0, self.orig.as_ptr()); } }
}

// Byte queue fed by poll/read: std::io::stdin()'s buffer would swallow escape
// sequence tails that poll() on fd 0 could never see again.
fn getc(q: &mut Vec<u8>, ms: i32) -> Option<u8> {
    if q.is_empty() {
        let mut f = [PollFd { fd: 0, events: 1, revents: 0 }];
        if unsafe { poll(f.as_mut_ptr(), 1, ms) } <= 0 { return None; }
        let mut b = [0u8; 64];
        let n = unsafe { read(0, b.as_mut_ptr(), 64) };
        if n <= 0 { return None; }
        q.extend_from_slice(&b[..n as usize]);
    }
    Some(q.remove(0))
}

#[derive(PartialEq)]
enum Key { L, R, D, U, Quit, Pause, Hold, Other }

fn key(q: &mut Vec<u8>, ms: i32) -> Option<Key> {
    Some(match getc(q, ms)? {
        b'\x1b' => match [getc(q, 20), getc(q, 20)] { // arrows: ESC [ X
            [Some(b'['), Some(x)] | [Some(b'O'), Some(x)] => match x {
                b'A' => Key::U, b'B' => Key::D, b'C' => Key::R, b'D' => Key::L, _ => Key::Other },
            _ => Key::Other,
        },
        b'a' | b'h' | b'A' | b'H' => Key::L,
        b'd' | b'l' | b'D' | b'L' => Key::R,
        b's' | b'S' | b'j' | b'J' => Key::D,
        b'w' | b'W' | b' ' | b'k' | b'K' => Key::U,
        b'q' | b'Q' | 3 => Key::Quit,
        b'p' | b'P' => Key::Pause,
        b'c' | b'C' => Key::Hold,
        _ => Key::Other,
    })
}

#[derive(Clone)]
struct P { k: usize, r: usize, x: i8, y: i8 }
impl P {
    fn new(k: usize) -> P { P { k, r: 0, x: 3, y: 0 } }
    fn cells(&self) -> [(i8, i8); 4] {
        let mut c = PIECES[self.k][self.r];
        for p in c.iter_mut() { p.0 += self.x; p.1 += self.y; }
        c
    }
}

struct G {
    b: [[u8; W as usize]; H as usize], cur: P, next: usize,
    hold: Option<usize>, held: bool,
    score: u32, lines: u32, level: u32, over: bool, bag: Vec<usize>,
}
impl G {
    fn new() -> G {
        let mut g = G { b: [[0; 10]; 20], cur: P::new(0), next: 0, hold: None, held: false,
            score: 0, lines: 0, level: 1, over: false, bag: Vec::new() };
        g.cur = P::new(g.draw());
        g.next = g.draw();
        g
    }
    fn draw(&mut self) -> usize { // 7-bag, clock-shuffled
        if self.bag.is_empty() {
            self.bag = (0..7).collect();
            for i in (1..7).rev() {
                let j = (SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos()
                    % (i as u32 + 1)) as usize;
                self.bag.swap(i, j);
            }
        }
        self.bag.pop().unwrap()
    }
    fn fits(&self, p: &P) -> bool {
        p.cells().iter().all(|&(x, y)|
            x >= 0 && x < W && y < H && (y < 0 || self.b[y as usize][x as usize] == 0))
    }
    fn mv(&mut self, dx: i8, dy: i8) -> bool {
        let mut p = self.cur.clone();
        p.x += dx; p.y += dy;
        if self.fits(&p) { self.cur = p; true } else { false }
    }
    fn rot(&mut self) {
        let mut p = self.cur.clone();
        p.r = (p.r + 1) % PIECES[p.k].len();
        for k in [0i8, -1, 1, -2, 2] { // wall kicks
            p.x += k;
            if self.fits(&p) { self.cur = p; return; }
            p.x -= k;
        }
    }
    fn lock(&mut self) {
        for &(x, y) in self.cur.cells().iter() {
            if y >= 0 { self.b[y as usize][x as usize] = self.cur.k as u8 + 1; }
        }
        let mut n = 0;
        let mut y = H;
        while y > 0 {
            y -= 1;
            if self.b[y as usize].iter().all(|&c| c != 0) {
                n += 1;
                self.b[..=y as usize].rotate_right(1);
                self.b[0] = [0; 10];
            }
        }
        if n > 0 {
            self.lines += n;
            self.score += [0, 100, 300, 500, 800][n as usize] * self.level;
            self.level = 1 + self.lines / 10;
        }
        self.cur = P::new(self.next);
        self.next = self.draw();
        self.held = false;
        self.over = !self.fits(&self.cur);
    }
    fn hold(&mut self) {
        if !self.held {
            self.held = true;
            let k = self.cur.k;
            self.cur = match self.hold.take() {
                Some(h) => P::new(h),
                None => { let n = self.next; self.next = self.draw(); P::new(n) }
            };
            self.hold = Some(k);
        }
    }
    fn at(&self, x: i8, y: i8) -> u8 {
        let k = self.cur.k as u8 + 1;
        if self.cur.cells().iter().any(|&(a, b)| a == x && b == y) { k } else { self.b[y as usize][x as usize] }
    }
    fn render(&self, paused: bool) -> String {
        let mut o = format!("\x1b[H\x1b[1;97m=====\x1b[0m {} \x1b[1;97m=====\x1b[0m\r\n\r\n\x1b[90m+------------+\x1b[0m\r\n",
            if paused { "PA3A" } else { "TETRIS" });
        for y in 0..H {
            o.push_str("\x1b[90m|\x1b[0m");
            for x in 0..W {
                let k = self.at(x, y);
                if k == 0 { o.push_str("  ") } else { o.push_str(COLORS[k as usize - 1]); o.push_str("[]\x1b[0m"); }
            }
            o.push_str("\x1b[90m|\x1b[0m");
            match y { // side info
                1 => o.push_str(" Next:"),
                2..=4 => for x in 0..5 {
                    let dy = y - 2;
                    if PIECES[self.next][0].iter().any(|&(a, b)| a == x && b == dy) {
                        o.push_str(COLORS[self.next]); o.push_str("[]\x1b[0m");
                    } else { o.push_str("  "); }
                },
                6 => o.push_str(&format!(" Score:{}", self.score)),
                7 => o.push_str(&format!(" Lines:{}", self.lines)),
                8 => o.push_str(&format!(" Level:{}", self.level)),
                10 => if let Some(h) = self.hold { o.push_str(&format!(" Hold:{}[]\x1b[0m", COLORS[h])); },
                15 => o.push_str(" <- -> move"),
                16 => o.push_str(" up=rotate, down=drop"),
                17 => o.push_str(" C=hold, P=pause, Q=quit"),
                _ => {}
            }
            o.push_str("\x1b[K\r\n");
        }
        o.push_str("\x1b[90m+------------+\x1b[0m\r\n\x1b[K");
        if self.over { o.push_str(&format!("\r\n\x1b[1;91mGAME OVER!\x1b[0m Score:{}. Any key=restart, Q=quit\r\n", self.score)); }
        o
    }
}

fn main() {
    let _raw = TermRaw::new();
    let mut o = std::io::stdout();
    let mut g = G::new();
    let mut paused = false;
    let mut last = Instant::now();
    let mut q = Vec::new();
    write!(o, "\x1b[?25l").unwrap();

    loop {
        if g.over {
            write!(o, "{}", g.render(false)).unwrap();
            o.flush().unwrap();
            match key(&mut q, 200) {
                Some(Key::Quit) => break,
                Some(Key::Other) => g = G::new(),
                _ => {}
            }
            continue;
        }
        let dt = Duration::from_millis((800u64.saturating_sub((g.level as u64 - 1) * 70)).max(80));
        let wait = dt.saturating_sub(last.elapsed()).as_millis().min(100) as i32;
        match key(&mut q, wait) {
            Some(Key::Quit) => break,
            Some(Key::Pause) => paused = !paused,
            Some(k) if !paused => match k {
                Key::L => { g.mv(-1, 0); }
                Key::R => { g.mv(1, 0); }
                Key::D => { if !g.mv(0, 1) { g.lock(); } g.score += 1; }
                Key::U => g.rot(),
                Key::Hold => g.hold(),
                Key::Other | Key::Quit | Key::Pause => {}
            },
            _ => {}
        }
        if !paused {
            if last.elapsed() >= dt {
                if !g.mv(0, 1) { g.lock(); }
                last = Instant::now();
            }
        }
        write!(o, "{}", g.render(paused)).unwrap();
        o.flush().unwrap();
    }
    write!(o, "\x1b[?25h\x1b[0m\x1b[2J\x1b[H").unwrap();
    o.flush().unwrap();
    println!("Thanks for playing! Score: {}", g.score);
}
