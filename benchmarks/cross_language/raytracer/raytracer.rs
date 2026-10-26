//! Graphics: a small ray tracer with shadows and reflections.
//!
//! Renders an N x N image of four spheres (one large one as the ground) lit by a
//! point light, with Lambert shading, hard shadows, and up to two reflection
//! bounces. Vectors are a small value class. Each pixel becomes three bytes, as in
//! a PPM file, and the program reports the rays cast, the hits, and a checksum of
//! the bytes. The float operations run in the same order in every language, so
//! the bytes match exactly.

const EPSILON: f64 = 0.0001;
const AMBIENT: f64 = 0.1;

#[derive(Clone, Copy)]
struct Vec3 {
    x: f64,
    y: f64,
    z: f64,
}

impl Vec3 {
    fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }

    fn add(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    fn sub(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    fn scale(self, k: f64) -> Vec3 {
        Vec3::new(self.x * k, self.y * k, self.z * k)
    }

    fn dot(self, other: Vec3) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    fn normalized(self) -> Vec3 {
        self.scale(1.0 / self.dot(self).sqrt())
    }
}

#[derive(Clone, Copy)]
struct Sphere {
    center: Vec3,
    radius: f64,
    color: Vec3,
    reflect: f64,
}

struct Tracer {
    spheres: Vec<Sphere>,
    light: Vec3,
    rays: i64,
    hits: i64,
}

impl Tracer {
    fn new() -> Self {
        let sphere = |center, radius, color, reflect| Sphere {
            center,
            radius,
            color,
            reflect,
        };
        Tracer {
            spheres: vec![
                sphere(
                    Vec3::new(0.0, -1.0, -3.0),
                    1.0,
                    Vec3::new(1.0, 0.2, 0.2),
                    0.3,
                ),
                sphere(
                    Vec3::new(2.0, 0.0, -4.0),
                    1.0,
                    Vec3::new(0.2, 0.2, 1.0),
                    0.5,
                ),
                sphere(
                    Vec3::new(-2.0, 0.0, -4.0),
                    1.0,
                    Vec3::new(0.2, 1.0, 0.2),
                    0.1,
                ),
                sphere(
                    Vec3::new(0.0, -5001.0, 0.0),
                    5000.0,
                    Vec3::new(1.0, 1.0, 0.8),
                    0.2,
                ),
            ],
            light: Vec3::new(5.0, 5.0, 0.0),
            rays: 0,
            hits: 0,
        }
    }

    fn intersect(&self, origin: Vec3, direction: Vec3, sphere: &Sphere) -> f64 {
        let oc = origin.sub(sphere.center);
        let b = oc.dot(direction);
        let c = oc.dot(oc) - sphere.radius * sphere.radius;
        let disc = b * b - c;
        if disc <= 0.0 {
            return -1.0;
        }
        let s = disc.sqrt();
        let mut t = -b - s;
        if t < EPSILON {
            t = -b + s;
        }
        if t < EPSILON {
            return -1.0;
        }
        t
    }

    fn trace(&mut self, origin: Vec3, direction: Vec3, depth: i64) -> Vec3 {
        self.rays += 1;
        let mut nearest = -1.0;
        let mut hit: i64 = -1;
        for i in 0..self.spheres.len() {
            let t = self.intersect(origin, direction, &self.spheres[i]);
            if t > 0.0 && (hit < 0 || t < nearest) {
                nearest = t;
                hit = i as i64;
            }
        }
        if hit < 0 {
            return Vec3::new(0.6, 0.8, 1.0).scale(0.5 + 0.5 * direction.y);
        }
        self.hits += 1;
        let sphere = self.spheres[hit as usize];
        let point = origin.add(direction.scale(nearest));
        let normal = point.sub(sphere.center).normalized();
        let to_light = self.light.sub(point);
        let distance = to_light.dot(to_light).sqrt();
        let light_dir = to_light.scale(1.0 / distance);
        let start = point.add(normal.scale(EPSILON));
        let mut shadowed = false;
        for other in &self.spheres {
            let t = self.intersect(start, light_dir, other);
            if t > 0.0 && t < distance {
                shadowed = true;
                break;
            }
        }
        let mut brightness = AMBIENT;
        if !shadowed {
            brightness += 0.0f64.max(normal.dot(light_dir));
        }
        let mut color = sphere.color.scale(brightness);
        if depth < 2 && sphere.reflect > 0.0 {
            let reflected = direction.sub(normal.scale(2.0 * direction.dot(normal)));
            let bounce = self.trace(start, reflected, depth + 1);
            color = color
                .scale(1.0 - sphere.reflect)
                .add(bounce.scale(sphere.reflect));
        }
        color
    }
}

fn to_byte(value: f64) -> i64 {
    (1.0f64.min(0.0f64.max(value)) * 255.0) as i64
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut tracer = Tracer::new();
    let origin = Vec3::new(0.0, 0.5, 1.0);
    let (mut checksum, mut total) = (0i64, 0i64);
    for y in 0..n {
        for x in 0..n {
            let px = 2.0 * (x as f64 + 0.5) / n as f64 - 1.0;
            let py = 1.0 - 2.0 * (y as f64 + 0.5) / n as f64;
            let direction = Vec3::new(px, py, -1.5).normalized();
            let color = tracer.trace(origin, direction, 0);
            for value in [to_byte(color.x), to_byte(color.y), to_byte(color.z)] {
                checksum = (checksum * 131 + value) % 1000000007;
                total += value;
            }
        }
    }
    println!(
        "raytracer size={n} rays={} hits={} mean={} checksum={checksum}",
        tracer.rays,
        tracer.hits,
        total / (n * n * 3)
    );
}
