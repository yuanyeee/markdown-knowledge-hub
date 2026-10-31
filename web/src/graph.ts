export type GraphNode={path:string;title:string};
export function layoutNodes(nodes:GraphNode[]) {
  return nodes.map((node,i)=>({...node,x:nodes.length===1?0:Math.cos(2*Math.PI*i/nodes.length),y:nodes.length===1?0:Math.sin(2*Math.PI*i/nodes.length)}));
}
