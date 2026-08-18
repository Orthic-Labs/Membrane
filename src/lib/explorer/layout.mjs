const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));

export function sphericalLayout(nodes, { radius = 240 } = {}) {
  const ordered = [...nodes].sort((a, b) => String(a.id).localeCompare(String(b.id)));
  const count = ordered.length;
  return ordered.map((node, index) => {
    if (count === 1) return { ...node, x: 0, y: 0, z: 0 };
    const y = 1 - (index / Math.max(1, count - 1)) * 2;
    const ring = Math.sqrt(Math.max(0, 1 - y * y));
    const theta = GOLDEN_ANGLE * index;
    return {
      ...node,
      x: Math.cos(theta) * ring * radius,
      y: y * radius,
      z: Math.sin(theta) * ring * radius,
    };
  });
}

export function projectOrb(nodes, { yaw = 0, pitch = 0, distance = 720 } = {}) {
  const cy = Math.cos(yaw), sy = Math.sin(yaw), cp = Math.cos(pitch), sp = Math.sin(pitch);
  return nodes.map((node) => {
    const x1 = node.x * cy - node.z * sy;
    const z1 = node.x * sy + node.z * cy;
    const y1 = node.y * cp - z1 * sp;
    const z2 = node.y * sp + z1 * cp;
    const scale = distance / Math.max(1, distance + z2);
    return { ...node, screenX: x1 * scale, screenY: y1 * scale, scale, depth: z2 };
  });
}
