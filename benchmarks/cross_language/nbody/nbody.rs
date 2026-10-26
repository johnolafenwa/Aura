//! Physics: the n-body simulation of the Jovian planets (Benchmarks Game).
//!
//! Integrates the Sun, Jupiter, Saturn, Uranus, and Neptune for N steps of 0.01
//! days with a simple symplectic integrator, keeping positions, velocities, and
//! masses in parallel float lists. Prints the system's energy before and after,
//! to nine decimals. Every language does the same float operations in the same
//! order, so the printed digits match exactly.

const PI: f64 = 3.141592653589793;
const SOLAR_MASS: f64 = 4.0 * PI * PI;
const DAYS_PER_YEAR: f64 = 365.24;

struct System {
    xs: Vec<f64>,
    ys: Vec<f64>,
    zs: Vec<f64>,
    vxs: Vec<f64>,
    vys: Vec<f64>,
    vzs: Vec<f64>,
    masses: Vec<f64>,
}

fn system() -> System {
    let mut s = System {
        xs: vec![
            0.0,
            4.84143144246472090e+00,
            8.34336671824457987e+00,
            1.28943695621391310e+01,
            1.53796971148509165e+01,
        ],
        ys: vec![
            0.0,
            -1.16032004402742839e+00,
            4.12479856412430479e+00,
            -1.51111514016986312e+01,
            -2.59193146099879641e+01,
        ],
        zs: vec![
            0.0,
            -1.03622044471123109e-01,
            -4.03523417114321381e-01,
            -2.23307578892655734e-01,
            1.79258772950371181e-01,
        ],
        vxs: vec![
            0.0,
            1.66007664274403694e-03,
            -2.76742510726862411e-03,
            2.96460137564761618e-03,
            2.68067772490389322e-03,
        ],
        vys: vec![
            0.0,
            7.69901118419740425e-03,
            4.99852801234917238e-03,
            2.37847173959480950e-03,
            1.62824170038242295e-03,
        ],
        vzs: vec![
            0.0,
            -6.90460016972063023e-05,
            2.30417297573763929e-05,
            -2.96589568540237556e-05,
            -9.51592254519715870e-05,
        ],
        masses: vec![
            1.0,
            9.54791938424326609e-04,
            2.85885980666130812e-04,
            4.36624404335156298e-05,
            5.15138902046611451e-05,
        ],
    };
    for i in 0..5 {
        s.vxs[i] = s.vxs[i] * DAYS_PER_YEAR;
        s.vys[i] = s.vys[i] * DAYS_PER_YEAR;
        s.vzs[i] = s.vzs[i] * DAYS_PER_YEAR;
        s.masses[i] = s.masses[i] * SOLAR_MASS;
    }
    s
}

fn offset_momentum(s: &mut System) {
    let (mut px, mut py, mut pz) = (0.0, 0.0, 0.0);
    for i in 0..5 {
        px += s.vxs[i] * s.masses[i];
        py += s.vys[i] * s.masses[i];
        pz += s.vzs[i] * s.masses[i];
    }
    s.vxs[0] = -px / SOLAR_MASS;
    s.vys[0] = -py / SOLAR_MASS;
    s.vzs[0] = -pz / SOLAR_MASS;
}

fn energy(s: &System) -> f64 {
    let mut e = 0.0;
    for i in 0..5 {
        e += 0.5 * s.masses[i] * (s.vxs[i] * s.vxs[i] + s.vys[i] * s.vys[i] + s.vzs[i] * s.vzs[i]);
        for j in i + 1..5 {
            let dx = s.xs[i] - s.xs[j];
            let dy = s.ys[i] - s.ys[j];
            let dz = s.zs[i] - s.zs[j];
            e -= s.masses[i] * s.masses[j] / (dx * dx + dy * dy + dz * dz).sqrt();
        }
    }
    e
}

fn advance(s: &mut System, dt: f64) {
    for i in 0..5 {
        for j in i + 1..5 {
            let dx = s.xs[i] - s.xs[j];
            let dy = s.ys[i] - s.ys[j];
            let dz = s.zs[i] - s.zs[j];
            let d2 = dx * dx + dy * dy + dz * dz;
            let mag = dt / (d2 * d2.sqrt());
            let mj = s.masses[j] * mag;
            s.vxs[i] -= dx * mj;
            s.vys[i] -= dy * mj;
            s.vzs[i] -= dz * mj;
            let mi = s.masses[i] * mag;
            s.vxs[j] += dx * mi;
            s.vys[j] += dy * mi;
            s.vzs[j] += dz * mi;
        }
    }
    for i in 0..5 {
        s.xs[i] += dt * s.vxs[i];
        s.ys[i] += dt * s.vys[i];
        s.zs[i] += dt * s.vzs[i];
    }
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut s = system();
    offset_momentum(&mut s);
    let before = energy(&s);
    for _ in 0..n {
        advance(&mut s, 0.01);
    }
    let after = energy(&s);
    println!("nbody steps={n} before={before:.9} after={after:.9}");
}
