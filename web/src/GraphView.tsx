import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import {
  GRAPH_LABEL,
  buildGraphLayout,
  type GraphNode,
  type NodeRole,
} from './graph';
import { read } from './transport';

type Data = {
  nodes: GraphNode[];
  edges: { source: string; target: string; call?: boolean }[];
  truncated: boolean;
};

type Transform = {
  x: number;
  y: number;
  scale: number;
};

function cardPalette(role: NodeRole, active: boolean) {
  if (role === 'center') {
    return {
      fill: '#fff9ec',
      stroke: active ? '#b87d14' : '#d59b34',
      strokeWidth: 2,
      accent: '#c18b2e',
      prefix: '#8c5d10',
      text: '#261a05',
    };
  }
  if (role === 'incoming') {
    return {
      fill: '#fffdfb',
      stroke: active ? '#b35d21' : '#e0bfaa',
      strokeWidth: active ? 1.8 : 1.2,
      accent: '#b35d21',
      prefix: '#8f4c20',
      text: '#1f2f43',
    };
  }
  if (role === 'outgoing') {
    return {
      fill: '#fbfdff',
      stroke: active ? '#246bb0' : '#b7cde3',
      strokeWidth: active ? 1.8 : 1.2,
      accent: '#246bb0',
      prefix: '#225891',
      text: '#1f2f43',
    };
  }
  return {
    fill: '#ffffff',
    stroke: active ? '#276453' : '#c6d6d0',
    strokeWidth: active ? 1.8 : 1.2,
    accent: '#327d68',
    prefix: '#566b7d',
    text: '#1f2f43',
  };
}

export function GraphView({
  project,
  center,
  expanded,
  onToggleExpand,
  onOpen,
}: {
  project: string;
  center?: string;
  expanded: boolean;
  onToggleExpand: () => void;
  onOpen: (path: string) => void;
}) {
  const [data, setData] = useState<Data | null>(null);
  const [error, setError] = useState('');
  const [summary, setSummary] = useState(false);
  const [hovered, setHovered] = useState<string | null>(null);
  const [viewportSize, setViewportSize] = useState({ width: 900, height: 480 });
  const [transform, setTransform] = useState<Transform>({
    x: 0,
    y: 0,
    scale: 1,
  });

  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    originX: number;
    originY: number;
    moved: boolean;
  } | null>(null);

  useEffect(() => {
    let active = true;
    setData(null);
    setError('');
    setHovered(null);
    read<Data>(summary ? 'graph-summary' : 'graph', project, {
      path: center || '',
    })
      .then((v) => {
        if (active) setData(v);
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
    };
  }, [project, center, summary]);

  const activeCenter = summary ? undefined : center;

  const layout = useMemo(
    () =>
      data
        ? buildGraphLayout(data.nodes, activeCenter, data.edges)
        : buildGraphLayout([]),
    [data, activeCenter],
  );

  const neighborMap = useMemo(() => {
    const map = new Map<string, Set<string>>();
    for (const n of layout.nodes) map.set(n.path, new Set([n.path]));
    for (const e of layout.edges) {
      map.get(e.source)?.add(e.target);
      map.get(e.target)?.add(e.source);
    }
    return map;
  }, [layout]);

  const fitView = useCallback(
    (w: number, h: number) => {
      if (!layout.nodes.length || w <= 0 || h <= 0) return;
      const { minX, minY, width: bw, height: bh } = layout.bounds;
      const scale = Math.min((w - 16) / bw, (h - 16) / bh, 1.15);
      if (!(scale > 0)) return;
      const cx = minX + bw / 2;
      const cy = minY + bh / 2;
      setTransform({
        x: w / 2 - cx * scale,
        y: h / 2 - cy * scale,
        scale,
      });
    },
    [layout],
  );

  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const updateSize = () => {
      const w = Math.floor(el.clientWidth);
      const h = Math.floor(el.clientHeight);
      if (w > 0 && h > 0) {
        setViewportSize((prev) =>
          prev.width === w && prev.height === h ? prev : { width: w, height: h },
        );
      }
    };
    updateSize();
    const observer = new ResizeObserver(updateSize);
    observer.observe(el);
    return () => observer.disconnect();
  }, [data, expanded]);

  useLayoutEffect(() => {
    fitView(viewportSize.width, viewportSize.height);
  }, [fitView, viewportSize.width, viewportSize.height]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const rect = el.getBoundingClientRect();
      const cursorX = event.clientX - rect.left;
      const cursorY = event.clientY - rect.top;
      const factor = event.deltaY < 0 ? 1.14 : 1 / 1.14;
      setTransform((prev) => {
        const nextScale = Math.max(0.12, Math.min(3.5, prev.scale * factor));
        const ratio = nextScale / prev.scale;
        return {
          scale: nextScale,
          x: cursorX - (cursorX - prev.x) * ratio,
          y: cursorY - (cursorY - prev.y) * ratio,
        };
      });
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [data]);

  function zoomBy(deltaFactor: number) {
    const cx = viewportSize.width / 2;
    const cy = viewportSize.height / 2;
    setTransform((prev) => {
      const nextScale = Math.max(0.12, Math.min(3.5, prev.scale * deltaFactor));
      const ratio = nextScale / prev.scale;
      return {
        scale: nextScale,
        x: cx - (cx - prev.x) * ratio,
        y: cy - (cy - prev.y) * ratio,
      };
    });
  }

  return (
    <section className="link-graph" aria-label="リンク図">
      <div className="link-graph-controls">
        <button type="button" onClick={() => setSummary((v) => !v)}>
          {summary ? '文書単位の図' : 'フォルダー集約図'}
        </button>
        <button
          type="button"
          aria-label="図を縮小"
          onClick={() => zoomBy(1 / 1.2)}
        >
          −
        </button>
        <output className="graph-zoom-readout" aria-label="図の倍率">
          {Math.round(transform.scale * 100)}%
        </output>
        <button
          type="button"
          aria-label="図を拡大"
          onClick={() => zoomBy(1.2)}
        >
          ＋
        </button>
        <button
          type="button"
          onClick={() => fitView(viewportSize.width, viewportSize.height)}
        >
          全体を表示
        </button>
        <button
          type="button"
          aria-pressed={expanded}
          onClick={onToggleExpand}
        >
          {expanded ? 'リンク図を元に戻す（Esc）' : 'リンク図を拡大'}
        </button>
        {!summary && center && (
          <span className="graph-legend" aria-label="凡例">
            <span className="legend-item incoming">← 左: 参照元・呼び出し元</span>
            <span className="legend-item center">■ 中央: 選択文書</span>
            <span className="legend-item outgoing">右: 参照先・呼び出し先 →</span>
          </span>
        )}
      </div>
      <h2>
        {summary
          ? 'フォルダー集約図'
          : center
            ? '選択文書のリンク図'
            : 'プロジェクトのリンク図'}
      </h2>
      <p className="hint">
        {summary
          ? '全体の文書をフォルダー階層別に集約しています。OMLの生成フォルダーは元仕様書名で表示します。'
          : center
            ? '左側に参照元（橙色）、右側に参照先（青色）を整列しています。太線は処理フローで確認した呼び出しです。カードをクリックするとWikiを開きます。'
            : '太線は処理フローで確認した呼び出しです。カードに重ねると関係先を強調し、クリックでWikiを開きます。'}
        ドラッグ・ホイールで移動／拡大できます。
      </p>
      {error && <p role="alert">{error}</p>}
      {!data && !error && <p>リンク図を読み込んでいます…</p>}

      <div
        className="graph-canvas"
        ref={containerRef}
        onPointerDown={(e) => {
          if (e.button !== 0) return;
          dragRef.current = {
            pointerId: e.pointerId,
            startX: e.clientX,
            startY: e.clientY,
            originX: transform.x,
            originY: transform.y,
            moved: false,
          };
        }}
        onPointerMove={(e) => {
          const drag = dragRef.current;
          if (!drag || drag.pointerId !== e.pointerId) return;
          const dx = e.clientX - drag.startX;
          const dy = e.clientY - drag.startY;
          if (Math.hypot(dx, dy) > 4) {
            drag.moved = true;
          }
          if (drag.moved) {
            setTransform((prev) => ({
              ...prev,
              x: drag.originX + dx,
              y: drag.originY + dy,
            }));
          }
        }}
        onPointerUp={() => {
          setTimeout(() => {
            dragRef.current = null;
          }, 0);
        }}
        onPointerCancel={() => {
          dragRef.current = null;
        }}
      >
        {layout.nodes.length > 0 && (
          <svg
            className="graph-svg"
            width={viewportSize.width}
            height={viewportSize.height}
            aria-label="関係図キャンバス"
          >
            <defs>
              <marker
                id="graph-arrow-incoming"
                viewBox="0 0 10 8"
                refX="9"
                refY="4"
                markerUnits="userSpaceOnUse"
                markerWidth="9"
                markerHeight="7.5"
                orient="auto"
              >
                <path d="M 0 0.5 L 9.5 4 L 0 7.5 z" fill="#b35d21" />
              </marker>
              <marker
                id="graph-arrow-outgoing"
                viewBox="0 0 10 8"
                refX="9"
                refY="4"
                markerUnits="userSpaceOnUse"
                markerWidth="9"
                markerHeight="7.5"
                orient="auto"
              >
                <path d="M 0 0.5 L 9.5 4 L 0 7.5 z" fill="#246bb0" />
              </marker>
              <marker
                id="graph-arrow-neutral"
                viewBox="0 0 10 8"
                refX="9"
                refY="4"
                markerUnits="userSpaceOnUse"
                markerWidth="8.5"
                markerHeight="7"
                orient="auto"
              >
                <path d="M 0 0.5 L 9.5 4 L 0 7.5 z" fill="#7c968e" />
              </marker>
            </defs>

            <g
              transform={`translate(${transform.x}, ${transform.y}) scale(${transform.scale})`}
            >
              <g className="graph-edges">
                {layout.edges.map((edge) => {
                  const touchesHovered =
                    hovered !== null &&
                    (edge.source === hovered || edge.target === hovered);
                  const dimmed = hovered !== null && !touchesHovered;
                  const effectiveRole = touchesHovered
                    ? edge.source === hovered
                      ? 'outgoing'
                      : 'incoming'
                    : edge.role;
                  const stroke =
                    effectiveRole === 'incoming'
                      ? '#b35d21'
                      : effectiveRole === 'outgoing'
                        ? '#246bb0'
                        : '#96ada6';
                  const markerId =
                    effectiveRole === 'incoming'
                      ? 'url(#graph-arrow-incoming)'
                      : effectiveRole === 'outgoing'
                        ? 'url(#graph-arrow-outgoing)'
                        : 'url(#graph-arrow-neutral)';
                  const strokeWidth = edge.call
                    ? touchesHovered
                      ? 2.6
                      : 2.2
                    : touchesHovered
                      ? 1.8
                      : 1.35;

                  return (
                    <g key={`${edge.source}->${edge.target}`}>
                      <path
                        d={edge.pathD}
                        fill="none"
                        stroke="#fcfefd"
                        strokeWidth={strokeWidth + 3.2}
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        opacity={dimmed ? 0.05 : 0.96}
                      />
                      <path
                        d={edge.pathD}
                        fill="none"
                        stroke={stroke}
                        strokeWidth={strokeWidth}
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        opacity={dimmed ? 0.14 : 0.95}
                        markerEnd={markerId}
                      />
                    </g>
                  );
                })}
              </g>

              <g className="graph-nodes">
                {layout.nodes.map((node, index) => {
                  const isHovered = hovered === node.path;
                  const connected =
                    hovered === null ||
                    Boolean(neighborMap.get(hovered)?.has(node.path));
                  const palette = cardPalette(node.role, isHovered);
                  const left = node.x - node.width / 2;
                  const top = node.y - node.height / 2;
                  const isCenter = node.role === 'center';
                  const mainFontSize = isCenter ? 13.5 : 12.5;
                  const mainBaseline0 = node.prefix
                    ? GRAPH_LABEL.mainBaselineWithPrefix
                    : GRAPH_LABEL.mainBaseline;

                  return (
                    <g
                      key={node.path}
                      transform={`translate(${left}, ${top})`}
                      className={`graph-card ${summary ? 'summary-card' : 'clickable-card'}`}
                      opacity={connected ? 1 : 0.24}
                      role={summary ? undefined : 'button'}
                      tabIndex={summary ? undefined : 0}
                      aria-label={node.title}
                      onMouseEnter={() => setHovered(node.path)}
                      onMouseLeave={() =>
                        setHovered((prev) => (prev === node.path ? null : prev))
                      }
                      onClick={() => {
                        if (summary || dragRef.current?.moved) return;
                        onOpen(node.path);
                      }}
                      onKeyDown={(e) => {
                        if (
                          !summary &&
                          (e.key === 'Enter' || e.key === ' ')
                        ) {
                          e.preventDefault();
                          onOpen(node.path);
                        }
                      }}
                    >
                      <title>{`${node.title}${summary ? '' : `\n${node.path}`}`}</title>
                      <rect
                        width={node.width}
                        height={node.height}
                        rx={8}
                        ry={8}
                        fill={palette.fill}
                        stroke={palette.stroke}
                        strokeWidth={palette.strokeWidth}
                      />
                      <rect
                        x={6}
                        y={8}
                        width={3.5}
                        height={Math.max(12, node.height - 16)}
                        rx={1.75}
                        fill={palette.accent}
                      />
                      <clipPath id={`graph-label-${index}`}>
                        <rect
                          x={14}
                          y={2}
                          width={Math.max(20, node.width - 22)}
                          height={Math.max(8, node.height - 4)}
                        />
                      </clipPath>
                      <g clipPath={`url(#graph-label-${index})`}>
                        {node.prefix && (
                          <text
                            x={GRAPH_LABEL.x}
                            y={GRAPH_LABEL.prefixBaseline}
                            fontSize={10.5}
                            fontWeight={600}
                            fill={palette.prefix}
                          >
                            {node.prefix}
                          </text>
                        )}
                        {node.mainLines.map((line, lineIndex) => (
                          <text
                            key={lineIndex}
                            x={GRAPH_LABEL.x}
                            y={
                              mainBaseline0 + lineIndex * GRAPH_LABEL.lineHeight
                            }
                            fontSize={mainFontSize}
                            fontWeight={isCenter ? 700 : 600}
                            fill={palette.text}
                          >
                            {line}
                          </text>
                        ))}
                      </g>
                    </g>
                  );
                })}
              </g>
            </g>
          </svg>
        )}
      </div>

      {data && (
        <>
          <p>
            {data.nodes.length}
            {summary ? 'グループ' : '文書'}・{data.edges.length}
            {summary ? '集約リンク' : 'リンク'}
            {data.truncated ? '（上限により一部を省略）' : ''}
          </p>
          {!data.nodes.length && <p>Markdown文書がありません。</p>}
          <details>
            <summary>
              {summary ? '図に含まれるグループ' : '図に含まれる文書'}
            </summary>
            {data.nodes.map((n) => (
              <button
                className="hit"
                key={n.path}
                disabled={summary}
                onClick={() => onOpen(n.path)}
              >
                {n.title} {!summary && <small>{n.path}</small>}
              </button>
            ))}
          </details>
        </>
      )}
    </section>
  );
}
