"""Graphics: a small ray tracer with shadows and reflections.

Renders an N x N image of four spheres (one large one as the ground) lit by a
point light, with Lambert shading, hard shadows, and up to two reflection
bounces. Vectors are a small value class. Each pixel becomes three bytes, as in
a PPM file, and the program reports the rays cast, the hits, and a checksum of
the bytes. The float operations run in the same order in every language, so
the bytes match exactly.
"""
import math
import sys

EPSILON = 0.0001
AMBIENT = 0.1


class Vec3:
    __slots__ = ("x", "y", "z")

    def __init__(self, x, y, z):
        self.x = x
        self.y = y
        self.z = z

    def add(self, other):
        return Vec3(self.x + other.x, self.y + other.y, self.z + other.z)

    def sub(self, other):
        return Vec3(self.x - other.x, self.y - other.y, self.z - other.z)

    def scale(self, k):
        return Vec3(self.x * k, self.y * k, self.z * k)

    def mul(self, other):
        return Vec3(self.x * other.x, self.y * other.y, self.z * other.z)

    def dot(self, other):
        return self.x * other.x + self.y * other.y + self.z * other.z

    def normalized(self):
        return self.scale(1.0 / math.sqrt(self.dot(self)))


class Sphere:
    def __init__(self, center, radius, color, reflect):
        self.center = center
        self.radius = radius
        self.color = color
        self.reflect = reflect


class Tracer:
    def __init__(self):
        self.spheres = [
            Sphere(Vec3(0.0, -1.0, -3.0), 1.0, Vec3(1.0, 0.2, 0.2), 0.3),
            Sphere(Vec3(2.0, 0.0, -4.0), 1.0, Vec3(0.2, 0.2, 1.0), 0.5),
            Sphere(Vec3(-2.0, 0.0, -4.0), 1.0, Vec3(0.2, 1.0, 0.2), 0.1),
            Sphere(Vec3(0.0, -5001.0, 0.0), 5000.0, Vec3(1.0, 1.0, 0.8), 0.2),
        ]
        self.light = Vec3(5.0, 5.0, 0.0)
        self.rays = 0
        self.hits = 0

    def intersect(self, origin, direction, sphere):
        oc = origin.sub(sphere.center)
        b = oc.dot(direction)
        c = oc.dot(oc) - sphere.radius * sphere.radius
        disc = b * b - c
        if disc <= 0.0:
            return -1.0
        s = math.sqrt(disc)
        t = -b - s
        if t < EPSILON:
            t = -b + s
        if t < EPSILON:
            return -1.0
        return t

    def trace(self, origin, direction, depth):
        self.rays += 1
        nearest = -1.0
        hit = -1
        for i in range(len(self.spheres)):
            t = self.intersect(origin, direction, self.spheres[i])
            if t > 0.0 and (hit < 0 or t < nearest):
                nearest = t
                hit = i
        if hit < 0:
            return Vec3(0.6, 0.8, 1.0).scale(0.5 + 0.5 * direction.y)
        self.hits += 1
        sphere = self.spheres[hit]
        point = origin.add(direction.scale(nearest))
        normal = point.sub(sphere.center).normalized()
        to_light = self.light.sub(point)
        distance = math.sqrt(to_light.dot(to_light))
        light_dir = to_light.scale(1.0 / distance)
        start = point.add(normal.scale(EPSILON))
        shadowed = False
        for other in self.spheres:
            t = self.intersect(start, light_dir, other)
            if t > 0.0 and t < distance:
                shadowed = True
                break
        brightness = AMBIENT
        if not shadowed:
            brightness += max(0.0, normal.dot(light_dir))
        color = sphere.color.scale(brightness)
        if depth < 2 and sphere.reflect > 0.0:
            reflected = direction.sub(normal.scale(2.0 * direction.dot(normal)))
            bounce = self.trace(start, reflected, depth + 1)
            color = color.scale(1.0 - sphere.reflect).add(bounce.scale(sphere.reflect))
        return color


def to_byte(value):
    return int(min(1.0, max(0.0, value)) * 255.0)


def main():
    n = int(sys.argv[1])
    tracer = Tracer()
    origin = Vec3(0.0, 0.5, 1.0)
    checksum = 0
    total = 0
    for y in range(n):
        for x in range(n):
            px = 2.0 * (x + 0.5) / n - 1.0
            py = 1.0 - 2.0 * (y + 0.5) / n
            direction = Vec3(px, py, -1.5).normalized()
            color = tracer.trace(origin, direction, 0)
            for value in (to_byte(color.x), to_byte(color.y), to_byte(color.z)):
                checksum = (checksum * 131 + value) % 1000000007
                total += value
    print(
        f"raytracer size={n} rays={tracer.rays} hits={tracer.hits} mean={total // (n * n * 3)} checksum={checksum}"
    )


if __name__ == "__main__":
    main()
