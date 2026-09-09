use std::f32::consts::{PI, TAU};
use std::io::{self, Write};

#[derive(Clone, Copy, Debug, Default)]
pub struct Vec3(pub f32, pub f32, pub f32);

impl Vec3 {
    fn add(self, b: Self) -> Self {
        Self(self.0 + b.0, self.1 + b.1, self.2 + b.2)
    }
    fn sub(self, b: Self) -> Self {
        Self(self.0 - b.0, self.1 - b.1, self.2 - b.2)
    }
    fn mul(self, k: f32) -> Self {
        Self(self.0 * k, self.1 * k, self.2 * k)
    }
    fn dot(self, b: Self) -> f32 {
        self.0 * b.0 + self.1 * b.1 + self.2 * b.2
    }
    fn cross(self, b: Self) -> Self {
        Self(
            self.1 * b.2 - self.2 * b.1,
            self.2 * b.0 - self.0 * b.2,
            self.0 * b.1 - self.1 * b.0,
        )
    }
    fn normal(self) -> Self {
        self.mul(self.dot(self).sqrt().max(0.000001).recip())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    Shell,
    Button,
    Base,
    Rubber,
    Metal,
    Ink,
    Accent,
}

impl Material {
    pub const ALL: [Self; 7] = [
        Self::Shell,
        Self::Button,
        Self::Base,
        Self::Rubber,
        Self::Metal,
        Self::Ink,
        Self::Accent,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::Shell => "graphite_shell",
            Self::Button => "main_buttons",
            Self::Base => "lower_chassis",
            Self::Rubber => "wheel_rubber",
            Self::Metal => "wheel_trim",
            Self::Ink => "insets_and_logo",
            Self::Accent => "indicator_preview",
        }
    }
    fn color(self, accent: [f32; 3]) -> Vec3 {
        match self {
            Self::Shell => Vec3(0.205, 0.215, 0.225),
            Self::Button => Vec3(0.225, 0.232, 0.245),
            Self::Base => Vec3(0.085, 0.09, 0.105),
            Self::Rubber => Vec3(0.065, 0.07, 0.075),
            Self::Metal => Vec3(0.42, 0.45, 0.49),
            Self::Ink => Vec3(0.027, 0.03, 0.036),
            Self::Accent => Vec3(accent[0], accent[1], accent[2]),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Vertex {
    pub position: Vec3,
    normal: Vec3,
}

pub struct Face {
    pub indices: [usize; 3],
    pub material: Material,
}

pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub faces: Vec<Face>,
}

fn width(z: f32) -> f32 {
    let end = if z > 0.0 {
        (1.0 - (z / 1.75).powf(2.6)).max(0.0).sqrt()
    } else {
        (1.0 - (z / 1.75).abs().powi(8)).max(0.0).powf(0.25)
    };
    let waist = 1.0 - 0.105 * (-((z + 0.15) / 0.65).powi(2)).exp();
    0.93 * end * waist
}

fn height(z: f32) -> f32 {
    let end = if z > 0.0 {
        (1.0 - (z / 1.75).powi(4)).max(0.0).sqrt()
    } else {
        (1.0 - (z / 1.75).abs().powi(16)).max(0.0).powf(0.22)
    };
    (0.47 + 0.46 * (-((z - 0.25) / 1.13).powi(2)).exp()) * end
}

fn top(x: f32, z: f32) -> f32 {
    let ratio = (x / width(z).max(0.001)).clamp(-1.0, 1.0);
    0.10 + height(z) * (1.0 - ratio * ratio).max(0.0).powf(0.44)
}

impl Mesh {
    fn surface(
        &mut self,
        nu: usize,
        nv: usize,
        material: impl Fn(f32, f32) -> Material,
        point: impl Fn(f32, f32) -> Vec3,
    ) {
        let start = self.vertices.len();
        for i in 0..=nu {
            for j in 0..=nv {
                let u = i as f32 / nu as f32;
                let v = j as f32 / nv as f32;
                let du = point((u + 0.0005).min(1.0), v).sub(point((u - 0.0005).max(0.0), v));
                let dv = point(u, (v + 0.0005).min(1.0)).sub(point(u, (v - 0.0005).max(0.0)));
                self.vertices.push(Vertex {
                    position: point(u, v),
                    normal: dv.cross(du).normal(),
                });
            }
        }
        for i in 0..nu {
            for j in 0..nv {
                let a = start + i * (nv + 1) + j;
                let b = a + nv + 1;
                let material = material((i as f32 + 0.5) / nu as f32, (j as f32 + 0.5) / nv as f32);
                for indices in [[a, a + 1, b], [a + 1, b + 1, b]] {
                    self.faces.push(Face { indices, material });
                }
            }
        }
    }

    fn ellipsoid(&mut self, center: Vec3, scale: Vec3, material: Material) {
        self.surface(
            32,
            16,
            |_, _| material,
            |u, v| {
                let a = u * TAU;
                let b = (v - 0.5) * PI;
                center.add(Vec3(
                    scale.0 * a.cos() * b.cos(),
                    scale.1 * b.sin(),
                    scale.2 * a.sin() * b.cos(),
                ))
            },
        );
    }

    pub fn msa10w() -> Self {
        let mut mesh = Self {
            vertices: Vec::new(),
            faces: Vec::new(),
        };
        mesh.surface(
            104,
            64,
            |u, v| {
                let z = (u * 2.0 - 1.0) * 1.75;
                let theta = v * PI;
                let x = width(z) * theta.cos();
                if z < -0.075 && theta > 0.13 && theta < PI - 0.13 {
                    let groove = x.abs() < 0.026
                        || (-1.33..-0.24).contains(&z) && x.abs() < 0.185
                        || (-0.115..-0.075).contains(&z);
                    if groove {
                        Material::Ink
                    } else {
                        Material::Button
                    }
                } else {
                    Material::Shell
                }
            },
            |u, v| {
                let z = (u * 2.0 - 1.0) * 1.75;
                let theta = v * PI;
                Vec3(
                    width(z) * theta.cos(),
                    0.10 + height(z) * theta.sin().max(0.0).powf(0.88),
                    z,
                )
            },
        );
        mesh.surface(
            104,
            16,
            |_, _| Material::Base,
            |u, v| {
                let z = (u * 2.0 - 1.0) * 1.75;
                let theta = PI + v * PI;
                Vec3(
                    width(z) * theta.cos() * 0.985,
                    0.075 + 0.20 * theta.sin(),
                    z,
                )
            },
        );
        mesh.surface(
            104,
            2,
            |_, _| Material::Ink,
            |u, v| {
                let z = (u * 2.0 - 1.0) * 1.75;
                let theta = v * PI;
                Vec3(width(z) * theta.cos(), 0.092, z)
            },
        );
        let wheel_y = top(0.0, -0.94) + 0.035;
        mesh.surface(
            96,
            12,
            |u, v| {
                if !(0.10..=0.90).contains(&v) {
                    Material::Metal
                } else if (u * 48.0).fract() < 0.42 {
                    Material::Button
                } else {
                    Material::Rubber
                }
            },
            |u, v| {
                let a = u * TAU;
                let shoulder = (1.0 - ((v - 0.5) * 2.0).abs().powi(8)).max(0.0).powf(0.15);
                let r = (0.205 + 0.008 * (a * 48.0).cos()) * (0.90 + 0.10 * shoulder);
                Vec3(
                    (v - 0.5) * 0.215,
                    wheel_y + r * a.sin(),
                    -0.94 + r * a.cos(),
                )
            },
        );
        for x in [-0.109, 0.109] {
            mesh.ellipsoid(
                Vec3(x, wheel_y, -0.94),
                Vec3(0.009, 0.182, 0.182),
                Material::Metal,
            );
        }
        mesh.ellipsoid(
            Vec3(0.0, top(0.0, -0.36) + 0.008, -0.36),
            Vec3(0.132, 0.032, 0.09),
            Material::Base,
        );
        mesh.ellipsoid(
            Vec3(0.0, top(0.0, -0.36) + 0.035, -0.36),
            Vec3(0.085, 0.009, 0.036),
            Material::Accent,
        );
        for z in [-0.66, -0.11] {
            mesh.ellipsoid(
                Vec3(-width(z) * 0.954, 0.385, z),
                Vec3(0.063, 0.068, 0.245),
                Material::Button,
            );
        }
        mesh.surface(
            80,
            3,
            |_, _| Material::Ink,
            |u, v| {
                let a = -u * TAU;
                let radius = 0.252 + v * 0.035;
                let x = a.cos() * radius;
                let z = 0.87 + a.sin() * radius;
                Vec3(x, top(x, z) + 0.003, z)
            },
        );
        for sign in [-1.0, 1.0] {
            mesh.surface(
                12,
                8,
                |_, _| Material::Ink,
                |u, v| {
                    let u = if sign < 0.0 { 1.0 - u } else { u };
                    let x = sign * (0.035 + u * 0.145);
                    let z = 0.83 + u * 0.075 + v * 0.08 * (1.0 - u);
                    Vec3(x, top(x, z) + 0.004, z)
                },
            );
            mesh.surface(
                2,
                2,
                |_, _| Material::Ink,
                |u, v| {
                    let u = if sign < 0.0 { 1.0 - u } else { u };
                    let x = sign * (0.23 + u * 0.10);
                    let z = 0.77 + v * 0.045;
                    Vec3(x, top(x, z) + 0.004, z)
                },
            );
        }
        mesh
    }

    pub fn write_obj(&self, mut output: impl Write) -> io::Result<()> {
        writeln!(output, "o MSA10W_approximate_visual_model")?;
        writeln!(output, "mtllib msa10w.mtl")?;
        for vertex in &self.vertices {
            let p = vertex.position;
            writeln!(output, "v {:.6} {:.6} {:.6}", p.0, p.1, p.2)?;
        }
        for vertex in &self.vertices {
            let n = vertex.normal;
            writeln!(output, "vn {:.6} {:.6} {:.6}", n.0, n.1, n.2)?;
        }
        for material in Material::ALL {
            writeln!(output, "g {}\nusemtl {}", material.name(), material.name())?;
            for face in self.faces.iter().filter(|f| f.material == material) {
                let [a, b, c] = face.indices.map(|i| i + 1);
                writeln!(output, "f {a}//{a} {b}//{b} {c}//{c}")?;
            }
        }
        output.flush()
    }

    pub fn write_mtl(mut output: impl Write) -> io::Result<()> {
        for material in Material::ALL {
            let c = material.color([0.66, 0.47, 1.0]);
            writeln!(
                output,
                "newmtl {}\nKd {} {} {}\nKs 0.25 0.25 0.25\nNs 70\nillum 2\n",
                material.name(),
                c.0,
                c.1,
                c.2
            )?;
        }
        output.flush()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            yaw: 0.62,
            pitch: 0.72,
            zoom: 1.0,
        }
    }
}

impl View {
    pub fn orbit(&mut self, horizontal: f32, vertical: f32) {
        if horizontal.is_finite() && vertical.is_finite() {
            self.yaw = (self.yaw + horizontal).rem_euclid(TAU);
            self.pitch = (self.pitch + vertical).clamp(-PI * 0.5, PI * 0.5);
        }
    }
    pub fn zoom_by(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.zoom = (self.zoom * factor).clamp(0.65, 1.8);
        }
    }
    fn rotate(self, p: Vec3) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let x = cy * p.0 + sy * p.2;
        let z = -sy * p.0 + cy * p.2;
        Vec3(x, cp * p.1 - sp * z, sp * p.1 + cp * z)
    }
}

#[derive(Clone, Copy)]
struct Projected {
    x: f32,
    y: f32,
    depth: f32,
    normal: Vec3,
}

pub fn render(mesh: &Mesh, view: View, accent: [f32; 3], width: usize, height: usize) -> Vec<u8> {
    let width = width.clamp(1, 1200);
    let height = height.clamp(1, 1000);
    let mut pixels = vec![0u8; width * height * 4];
    let mut depth = vec![f32::NEG_INFINITY; width * height];
    let focal = width.min(height) as f32 * 2.1 * view.zoom;
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|v| {
            let p = view.rotate(v.position.sub(Vec3(0.0, 0.40, 0.0)));
            let distance = 6.5 - p.2;
            Projected {
                x: width as f32 * 0.5 + focal * p.0 / distance,
                y: height as f32 * 0.48 - focal * p.1 / distance,
                depth: distance.recip(),
                normal: view.rotate(v.normal),
            }
        })
        .collect();
    let light = Vec3(-0.45, 0.75, 0.85).normal();
    let half = light.add(Vec3(0.0, 0.0, 1.0)).normal();
    for face in &mesh.faces {
        let [a, b, c] = face.indices.map(|i| vertices[i]);
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if area.abs() < 0.00001 {
            continue;
        }
        let min_x = a.x.min(b.x).min(c.x).floor().clamp(0.0, width as f32) as usize;
        let max_x = a.x.max(b.x).max(c.x).ceil().clamp(0.0, width as f32) as usize;
        let min_y = a.y.min(b.y).min(c.y).floor().clamp(0.0, height as f32) as usize;
        let max_y = a.y.max(b.y).max(c.y).ceil().clamp(0.0, height as f32) as usize;
        let base = face.material.color(accent);
        for y in min_y..max_y {
            for x in min_x..max_x {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let u = ((b.x - px) * (c.y - py) - (b.y - py) * (c.x - px)) / area;
                let v = ((c.x - px) * (a.y - py) - (c.y - py) * (a.x - px)) / area;
                let w = 1.0 - u - v;
                if u < -0.0001 || v < -0.0001 || w < -0.0001 {
                    continue;
                }
                let z = u * a.depth + v * b.depth + w * c.depth;
                let index = y * width + x;
                if z <= depth[index] {
                    continue;
                }
                depth[index] = z;
                let mut n = a
                    .normal
                    .mul(u * a.depth)
                    .add(b.normal.mul(v * b.depth))
                    .add(c.normal.mul(w * c.depth))
                    .normal();
                if n.2 < 0.0 {
                    n = n.mul(-1.0);
                }
                let diffuse = 0.34 + 0.72 * n.dot(light).max(0.0);
                let shine = n.dot(half).max(0.0).powi(44)
                    * if face.material == Material::Metal {
                        0.55
                    } else {
                        0.12
                    };
                let rim = (1.0 - n.2.max(0.0)).powi(3) * 0.11;
                let color = if face.material == Material::Accent {
                    base.mul(0.90).add(Vec3(0.10, 0.10, 0.10))
                } else {
                    base.mul(diffuse)
                        .add(Vec3(shine + rim * 0.7, shine + rim * 0.8, shine + rim))
                };
                let channel = |c: f32| (c.clamp(0.0, 1.0) * 255.0) as u32;
                let value =
                    0xff000000 | channel(color.0) << 16 | channel(color.1) << 8 | channel(color.2);
                pixels[index * 4..index * 4 + 4].copy_from_slice(&value.to_ne_bytes());
            }
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry_is_finite_and_exports_real_triangles() {
        let mesh = Mesh::msa10w();
        assert!(mesh.faces.len() > 10000);
        for vertex in &mesh.vertices {
            let Vec3(x, y, z) = vertex.position;
            assert!(x.is_finite() && y.is_finite() && z.is_finite());
            assert!(x.abs() < 1.1 && (-0.3..1.5).contains(&y) && z.abs() <= 1.75);
        }
        for face in &mesh.faces {
            assert!(face.indices.iter().all(|i| *i < mesh.vertices.len()));
        }
        let mut obj = Vec::new();
        mesh.write_obj(&mut obj).unwrap();
        let obj = String::from_utf8(obj).unwrap();
        assert_eq!(
            obj.lines().filter(|l| l.starts_with("f ")).count(),
            mesh.faces.len()
        );
        assert!(obj.contains("usemtl indicator_preview"));
        assert_eq!(obj, include_str!("../../../assets/msa10w/msa10w.obj"));
        let mut materials = Vec::new();
        Mesh::write_mtl(&mut materials).unwrap();
        assert_eq!(
            materials,
            include_bytes!("../../../assets/msa10w/msa10w.mtl")
        );
    }
    #[test]
    fn camera_is_bounded_and_renderer_changes_with_rotation() {
        let mut view = View::default();
        view.orbit(f32::NAN, 0.0);
        assert_eq!(view, View::default());
        view.zoom_by(100.0);
        assert_eq!(view.zoom, 1.8);
        view.orbit(100.0, 100.0);
        assert_eq!(view.pitch, PI * 0.5);
        let mesh = Mesh::msa10w();
        let initial = render(&mesh, View::default(), [0.6, 0.4, 1.0], 160, 120);
        let rotated = render(&mesh, view, [0.6, 0.4, 1.0], 160, 120);
        assert_ne!(initial, rotated);
        assert!(initial.iter().any(|b| *b != 0));
        assert_eq!(render(&mesh, view, [1.0; 3], 0, 0).len(), 4);
    }
}
