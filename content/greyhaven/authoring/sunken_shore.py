"""Blender source for Stillwater's continuous rock cover above the existing Sunken Shrine.
Coarse, closed collision geometry in game metres; no rendering asset or kit replacement.
"""
import bpy, bmesh, math, json, pathlib
ROOT=pathlib.Path(__file__).resolve().parents[3]
OUT=ROOT/'crates/mmorpg-core/content/collision/sunken-shore.json'
NAME='Stillwater shrine rock shelf'
old=bpy.data.collections.get(NAME)
linked=[s for s in bpy.data.scenes if old and old.name in s.collection.children]
if old:
    for obj in list(old.objects):bpy.data.objects.remove(obj,do_unlink=True)
    bpy.data.collections.remove(old)
collection=bpy.data.collections.new(NAME)
for scene in set(linked+[bpy.context.scene]):scene.collection.children.link(collection)
mat=bpy.data.materials.get('Stillwater shelf preview')
if mat is None:
    mat=bpy.data.materials.new('Stillwater shelf preview');mat.use_nodes=True
    shader=next(n for n in mat.node_tree.nodes if n.type=='BSDF_PRINCIPLED')
    shader.inputs['Base Color'].default_value=(.31,.34,.24,1)
    shader.inputs['Roughness'].default_value=.95
meshes=[]
# Every neighbouring cell samples the same cross-section at its shared edge. The cover
# widens over the existing air room, then joins the flat shore with no staircase seams.
def section(x):
    width=6+4*max(0,min((x-76)/8,(100-x)/8,1))
    fade=max(0,min((84-x)/12,1))
    base=min(11,8+(x-56)*.25)
    points=[]
    for side in [-1,-.7,-.35,0,.35,.7,1]:
        z=-54+side*width
        ridge=fade*(1-abs(side))*(.65+.45*math.sin(x*.43)+.18*math.cos(z*.7))
        points.append((x,base+ridge,z))
    return points
for cell,x in enumerate(range(56,124,4)):
    ceiling=7 if 84<=x<92 else 5
    # Mid-cell sections add broad fractures, while edge samples match exactly in centimetres.
    upper=[p for along in [x,x+2,x+4] for p in section(along)]
    n=7;vs=[(px,ceiling,pz) for px,_,pz in upper]+upper
    offset=len(upper);faces=[]
    for row in range(2):
        for j in range(n-1):
            a=row*n+j;b=a+n
            faces.extend([(a,a+1,b+1,b),(offset+a,offset+b,offset+b+1,offset+a+1)])
    outline=list(range(n))+[2*n-1,3*n-1]+list(range(3*n-2,2*n-1,-1))+[n]
    faces += [(a,b,b+offset,a+offset) for a,b in zip(outline,outline[1:]+outline[:1])]
    data=bpy.data.meshes.new('Shrine shelf cell '+str(cell))
    data.from_pydata([(px,-pz,py) for px,py,pz in vs],[],faces);data.update()
    bm=bmesh.new();bm.from_mesh(data);bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces));bm.to_mesh(data);bm.free()
    data.calc_loop_triangles();data.materials.append(mat)
    obj=bpy.data.objects.new('Stillwater continuous shelf '+str(580+cell),data)
    obj['collider_id']=580+cell;collection.objects.link(obj)
    meshes.append(dict(id=580+cell,position=[0,0,0],
        vertices=[[round(v.co.x*100),round(v.co.z*100),round(-v.co.y*100)] for v in data.vertices],
        triangles=[list(t.vertices) for t in data.loop_triangles]))
OUT.write_text(json.dumps(meshes,separators=(',',':'))+'\n')
print('Exported',len(meshes),'joined shelf cells, preserving the flooded route and air room:',OUT)
