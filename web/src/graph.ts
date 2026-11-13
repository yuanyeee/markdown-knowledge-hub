export type GraphNode = { path: string; title: string };
export type PositionedGraphNode = GraphNode & { x: number; y: number };

function arrangeConcentric(
  items: GraphNode[],
  baseRadius: number,
  ringStep: number,
  firstRingCapacity: number,
): PositionedGraphNode[] {
  const result: PositionedGraphNode[] = [];
  let cursor = 0;
  let ring = 0;

  while (cursor < items.length) {
    const capacity = firstRingCapacity * (ring + 1);
    const slice = items.slice(cursor, cursor + capacity);
    const radius = baseRadius + ring * ringStep;
    const phase = ring % 2 === 1 ? Math.PI / slice.length : 0;

    slice.forEach((node, idx) => {
      const angle = (2 * Math.PI * idx) / slice.length + phase;
      result.push({
        ...node,
        x: radius * Math.cos(angle),
        y: radius * Math.sin(angle),
      });
    });

    cursor += slice.length;
    ring += 1;
  }

  return result;
}

export function layoutNodes(
  nodes: GraphNode[],
  center?: string,
): PositionedGraphNode[] {
  if (nodes.length === 0) return [];
  if (nodes.length === 1) {
    return [{ ...nodes[0], x: 0, y: 0 }];
  }

  if (center && nodes.some((n) => n.path === center)) {
    const neighbors = nodes.filter((n) => n.path !== center);
    const positionedNeighbors = arrangeConcentric(neighbors, 0.75, 0.55, 16);
    const neighborMap = new Map(positionedNeighbors.map((n) => [n.path, n]));
    return nodes.map((node) =>
      node.path === center
        ? { ...node, x: 0, y: 0 }
        : neighborMap.get(node.path)!,
    );
  }

  if (nodes.length <= 20) {
    return nodes.map((node, i) => ({
      ...node,
      x: Math.cos((2 * Math.PI * i) / nodes.length),
      y: Math.sin((2 * Math.PI * i) / nodes.length),
    }));
  }

  return arrangeConcentric(nodes, 0.55, 0.5, 14);
}
