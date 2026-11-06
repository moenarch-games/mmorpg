"""Blender authoring source: rounded Redbrand mound with a concealed north entrance.
Simplified closed surfaces are persisted for Rapier; the shared rock presenter textures them.
Run in Blender's Python console with this file's __file__ set to its real path.
"""
import bpy, bmesh, math, json, pathlib
from mathutils import Vector
ROOT=pathlib.Path(__file__).resolve().parents[3]
OUT=ROOT/'crates/mmorpg-core/content/collision/redbrand.json'
NAME='Redbrand rounded north entrance'
old=bpy.data.collections.get(NAME)
linked_scenes=[s for s in bpy.data.scenes if old and old.name in s.collection.children]
if old:
    for o in list(old.objects):bpy.data.objects.remove(o,do_unlink=True)
    bpy.data.collections.remove(old)
collection=bpy.data.collections.new(NAME)
for scene in set(linked_scenes+[bpy.context.scene]):scene.collection.children.link(collection)
material=bpy.data.materials.get('Redbrand limestone authoring preview')
if not material:
    material=bpy.data.materials.new('Redbrand limestone authoring preview');material.use_nodes=True
    p=next(n for n in material.node_tree.nodes if n.type=='BSDF_PRINCIPLED');p.inputs['Base Color'].default_value=(.32,.30,.26,1);p.inputs['Roughness'].default_value=.94
meshes=[]

def record(id,name,vertices,faces):
    data=bpy.data.meshes.new(name)
    data.from_pydata([(x,-z,y+11) for x,y,z in vertices],[],faces);data.update()
    bm=bmesh.new();bm.from_mesh(data);bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces));bm.to_mesh(data);bm.free()
    data.calc_loop_triangles();triangles=[]
    for tri in data.loop_triangles:
        a,b,c=(data.vertices[i].co for i in tri.vertices)
        if (b-a).cross(c-a).length_squared>1e-9:triangles.append(list(tri.vertices))
    data.materials.append(material);obj=bpy.data.objects.new(name,data);collection.objects.link(obj);obj['collider_id']=id
    meshes.append(dict(id=id,position=[0,1100,0],vertices=[[round(x*100),round((z-11)*100),round(-y*100)] for x,y,z in (v.co for v in data.vertices)],triangles=triangles))

def outer(x,z):
    dx,dz=x-64,z+138
    radius=math.sqrt((dx/26)**2+(dz/16)**2)
    angle=math.atan2(dz,dx)
    fracture=1+.045*math.sin(angle*7)+.025*math.cos(angle*11)
    return (64+dx/radius*fracture,-138+dz/radius*fracture)

def section(x,z):
    ox,oz=outer(x,z);dx,dz=ox-64,oz+138
    # Inner contact planes stay clear at body height. Above them, uneven recesses break
    # the straight chamber walls without shrinking the combat floor or its approaches.
    distance=math.hypot(ox-x,oz-z)
    recess=.18+.58*(.5+.5*math.sin(x*.83+z*.57))
    rx,rz=(ox-x)/distance*recess,(oz-z)/distance*recess
    return [(x,-11,z),(ox,-11,oz),(ox,-1,oz),
        (64+dx*.92,2.8,-138+dz*.92),(64+dx*.82,4.8,-138+dz*.82),
        (x+(ox-x)*.05,4.5,z+(oz-z)*.05),
        (x+rx,3.1+.25*math.sin(x*.4-z*.6),z+rz),(x,1.9,z),(x,0,z)]

# Clockwise inner perimeter; split the north wall at a four-metre opening (x 62..66).
wall_specs=[(540,'North west rock',[(x,-144) for x in range(48,63,2)]),
    (646,'North east rock',[(x,-144) for x in range(66,81,2)]),
    (544,'East rock',[(80,z) for z in range(-144,-131,2)]),
    (542,'South east rock',[(x,-132) for x in range(80,59,-2)]),
    (647,'Sealed old south mouth',[(60,-132),(58,-132),(56,-132)]),
    (541,'South west rock',[(x,-132) for x in range(56,47,-2)]),
    (543,'West rock',[(48,z) for z in range(-132,-145,-2)])]
for id,name,points in wall_specs:
    vs=[p for x,z in points for p in section(x,z)];n=9
    fs=[tuple(range(n-1,-1,-1)),tuple(range(len(vs)-n,len(vs)))]
    fs += [(i*n+j,i*n+(j+1)%n,(i+1)*n+(j+1)%n,(i+1)*n+j) for i in range(len(points)-1) for j in range(n)]
    record(id,name,vs,fs)

# The divider has fractured shoulders instead of a two-box silhouette. The 4 m passage stays.
def box(id,name,minx,maxx,minz,maxz,low,high):
    vs=[(x,y,z) for y in [low,high] for x,z in [(minx,minz),(maxx,minz),(maxx,maxz),(minx,maxz)]]
    fs=[(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)]
    record(id,name,vs,fs)
def rib(id,name,z0,z1):
    # Chamfer the old box corners within its footprint; the four-metre passage widens
    # at its shoulders without changing the floor, shelf or sealed outside perimeter.
    outline=[(68.65,z0),(71.4,z0),(72,z0+.65),(72,z1-.6),
        (71.35,z1),(68.55,z1),(68,z1-.7),(68,z0+.5)]
    vs=[];n=len(outline)
    for row,y in enumerate([-11,0,1.9,3.15,4.5]):
        for j,(x,z) in enumerate(outline):
            taper=0 if row<3 else (.14+.15*math.sin(j*2.4+id))
            vs.append((x+(70-x)*taper,y,z+((z0+z1)/2-z)*taper))
    fs=[tuple(range(n-1,-1,-1)),tuple(range(4*n,5*n))]
    fs += [(r*n+j,r*n+(j+1)%n,(r+1)*n+(j+1)%n,(r+1)*n+j) for r in range(4) for j in range(n)]
    record(id,name,vs,fs)
rib(545,'North chamber fractured rib',-144,-140)
rib(546,'South chamber fractured rib',-136,-132)

# Closed, irregular domed roof: flat underside over usable rooms, rising craggy outer crown.
perimeter=([(x,-144) for x in range(48,80,2)]+[(80,z) for z in range(-144,-132,2)]
    +[(x,-132) for x in range(80,48,-2)]+[(48,z) for z in range(-132,-144,-2)])
ring=[outer(x,z) for x,z in perimeter];n=len(ring);vs=[]
for fraction,height in [(1,4.5),(1,4.8),(.78,9.8),(.47,14.1),(.18,16.3)]:
    for i,(x,z) in enumerate(ring):
        # Unequal ribs and an offset summit keep this a rock mass, not a regular dome.
        jiggle=0 if height<5 else fraction*(2.8*math.sin(i*math.tau/n*3)+.8*math.cos(i*math.tau/n*5))
        vs.append((64-(1-fraction)+(x-64)*.82*fraction,height+jiggle,-138+.5*(1-fraction)+(z+138)*.82*fraction))
vs.append((62.2,17.3,-137.8));top=len(vs)-1
fs=[tuple(range(n-1,-1,-1))]
fs += [(r*n+i,r*n+(i+1)%n,(r+1)*n+(i+1)%n,(r+1)*n+i) for r in range(4) for i in range(n)]
fs += [(4*n+i,4*n+(i+1)%n,top) for i in range(n)]
record(547,'Rounded Redbrand crag roof',vs,fs)
# A rough, shallow vault gives the far-side mouth a natural arch rather than a flat lintel.
profile=[(58,3.0),(60.5,3.4),(62.5,4.25),(64,4.5),(65.5,4.15),(67.5,3.35),(70,3.0)]
vs=[(x,y,z) for z in [-153.5,-150] for x,y in profile]
# Follow the arch with a broken upper edge instead of a rectangular lintel slab.
vs += [(x,y+1.1+.22*math.sin(x*2.1),z) for z in [-153.5,-150] for x,y in profile]
n=len(profile);fs=[]
for i in range(n-1):
    fs += [(i,i+1,n+i+1,n+i),(2*n+i,3*n+i,3*n+i+1,2*n+i+1),
        (i,2*n+i,2*n+i+1,i+1),(n+i,n+i+1,3*n+i+1,3*n+i)]
fs += [(0,n,3*n,2*n),(n-1,2*n-1,4*n-1,3*n-1)]
record(648,'North entrance rough stone arch',vs,fs)
# Existing floors remain individually authored: they support without any heightfield.
box(640,'Outer chamber floor',48,68,-144,-132,-.20,0)
box(641,'Chamber passage floor',68,72,-140,-136,-.20,0)
box(642,'East chamber floor',72,80,-144,-132,-.20,0)
# A broad-to-narrow north mouth and a short exposed apron meet the terrain at 11 m.
vs=[(x,y,z) for y in [-.20,0] for x,z in [(58,-156),(70,-156),(66,-144),(62,-144)]]
record(643,'North entrance floor',vs,[(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)])
# Move the raised shelf to the south wall so the north entrance opens onto the lower floor.
vs=[(x,y,z) for y in [-.2,0] for x,z in [(49,-134),(56,-134),(56,-132),(49,-132)]]
for i in [5,6]:vs[i]=(vs[i][0],2.2,vs[i][2])
record(644,'South shelf slope',vs,[(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)])
box(645,'South raised shelf',56,67,-134,-132,2,2.2)
# Small ceiling formations keep full walking headroom above the lower rooms. None hang
# over the elevated south shelf; their closed coarse meshes remain authoritative.
for id,x,z,tip,radius in [(660,57,-141.5,3.4,.55),(661,64,-136.8,3.7,.4),
    (662,73.5,-141.2,3.3,.6),(663,77.8,-134.5,3.6,.48)]:
    n=9;vs=[]
    for y,r in [(4.65,radius),(4.1,radius*.7)]:
        for j in range(n):
            a=j*math.tau/n;w=1+.10*math.sin(j*7+id)
            vs.append((x+r*w*math.sin(a),y,z+r*w*math.cos(a)))
    vs.append((x+.06,tip,z-.04))
    fs=[tuple(range(n-1,-1,-1))]+[(j,(j+1)%n,(j+1)%n+n,j+n) for j in range(n)]
    fs += [(n+j,n+(j+1)%n,2*n) for j in range(n)]
    record(id,'Cave ceiling formation '+str(id),vs,fs)

# Fractured stone jambs support the broad north arch without narrowing its central route.
for id,x,z in [(664,58.8,-153.0),(665,69.2,-152.8)]:
    n=9;vs=[]
    for y,r in [(-.3,.9),(.5,1.0),(2.9,.7),(4.8,.3)]:
        for j in range(n):
            a=j*math.tau/n;w=1+.13*math.sin(j*11+id)
            vs.append((x+r*w*math.sin(a),y+.08*math.cos(j*3),z+r*w*math.cos(a)*1.1))
    fs=[tuple(range(n-1,-1,-1)),tuple(range(3*n,4*n))]
    fs += [(r*n+j,r*n+(j+1)%n,(r+1)*n+(j+1)%n,(r+1)*n+j) for r in range(3) for j in range(n)]
    record(id,'North mouth fractured jamb '+str(id),vs,fs)

# Redbrand Hollow: unequal crests and saddles over the established lower contact footprints.
# These remain modest static collision meshes; the game supplies the existing rock PBR.
moss=bpy.data.materials.get('Hollow moss - Blender preview')
if not moss:
    moss=bpy.data.materials.new('Hollow moss - Blender preview');moss.use_nodes=True
    node=next(n for n in moss.node_tree.nodes if n.type=='BSDF_PRINCIPLED')
    node.inputs['Base Color'].default_value=(.20,.26,.13,1);node.inputs['Roughness'].default_value=.98
for id,(x0,x1,z0,z1,h) in {
    400:(-32,-22,-120,-80,12),401:(-22,-16,-84,-80,8),402:(22,32,-120,-74,12),
    403:(16,22,-96,-86,9),404:(-22,22,-120,-110,14),405:(-22,-8,-110,-104,10),406:(8,22,-110,-105,10),
}.items():
    cx,cz=(x0+x1)/2,(z0+z1)/2;hx,hz=(x1-x0)/2,(z1-z0)/2
    outline=[]
    for j in range(40):
        a=j*math.tau/40;s,c=math.sin(a),math.cos(a)
        outline.append((cx+hx*math.copysign(abs(s)**.28,s),cz+hz*math.copysign(abs(c)**.28,c)))
    if id==402:
        # A short, level shelf supports the existing west-face rope at z=-100.5.
        # Added samples preserve the face here without imposing one flat crown on the ridge.
        outline[11:11]=[(32,-100.5),(32,-104)]
        outline[32:32]=[(22,-104),(22,-100.5)]
    n=len(outline);vs=[]
    for row,(width,along,fraction) in enumerate([(1,1,0),(1,1,0),(.96,.98,.35),(.76,.95,.72),(.43,.92,1)]):
        for j,(x,z) in enumerate(outline):
            t=(z-cz)/hz if hz>hx else (x-cx)/hx
            end=.45+.55*(1-abs(t)**6)
            crest=max(3.2,h*(.86+.40*math.sin(t*5.3+id*.71)+.20*math.cos(t*10.1-id*.37))*end)
            if id==402:
                crest=min(12,crest)
                if abs(z+100.5)<8:crest=12
            y=(0 if id==402 else -1) if row==0 else (2 if row==1 else 2+(crest-2)*fraction)
            w=width if row<2 else width*(1+.10*math.sin(j*2.7+id))
            sx,sz=(w,along) if hz>hx else (along,w)
            if id==402 and x<cx:sx=1
            vs.append((cx+(x-cx)*sx,y,cz+(z-cz)*sz))
    fs=[tuple(range(n-1,-1,-1))]
    fs += [(r*n+j,r*n+(j+1)%n,(r+1)*n+(j+1)%n,(r+1)*n+j) for r in range(4) for j in range(n)]
    if id==402:
        # Split the cap at the rope landing: this short plateau must stay flat when Blender
        # triangulates the non-planar crown on either side of it.
        for cap in [list(range(12,33)),[10,11,12,32,33,34],list(range(34,n))+list(range(11))]:
            fs.append(tuple(4*n+j for j in cap))
    else:fs.append(tuple(range(4*n,5*n)))
    record(id,'Hollow broken ridge '+str(id),vs,fs)
    obj=collection.objects.get('Hollow broken ridge '+str(id));obj.data.materials.append(moss)
    for face in obj.data.polygons:face.material_index=1 if face.normal.z>.60 else 0
OUT.write_text(json.dumps(sorted(meshes,key=lambda m:m['id']),separators=(',',':'))+'\n')
print('Exported',len(meshes),'closed rock/floor meshes, north mouth and south shelf:',OUT)
