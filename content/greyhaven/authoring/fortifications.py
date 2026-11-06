"""Run in Blender: Greyhaven keep, timber defences, Millbrook buildings and Hollow camp.
Uses Blender's glTF exporter; creates no renderer/runtime dependency.
Coordinates passed to helpers are game metres (Y up, +Z south).
"""
import bpy, math, json, pathlib, hashlib
import numpy as np
from mathutils import Vector
ROOT = pathlib.Path(__file__).resolve().parents[3]
OUT = ROOT / 'web/public/authored/greyhaven-fort'
OUT.mkdir(parents=True, exist_ok=True)
COLLISION = ROOT / 'crates/mmorpg-core/content/collision/greyhaven-fort.json'
COLLECTION = 'Greyhaven authored fortifications'
old = bpy.data.collections.get(COLLECTION)
if old:
    for o in list(old.objects): bpy.data.objects.remove(o, do_unlink=True)
    bpy.data.collections.remove(old)
collection = bpy.data.collections.new(COLLECTION)
bpy.context.scene.collection.children.link(collection)
objects, collision = [], []
rng = np.random.default_rng(513)
N = 512
v,u = np.mgrid[0:N,0:N] / N
# Hand-authored tileable coursed stone: staggered blocks, rough mortar, fine grain.
row = np.floor(v*8)
xx = (u*4 + (row%2)*.5) % 1
yy = (v*8) % 1
edge = np.minimum(np.minimum(xx,1-xx)*2, np.minimum(yy,1-yy))
noise = rng.random((N,N))
stone_height = np.clip((edge-.026)*18,0,1) * (.82+.14*noise)
block = np.sin(np.floor(u*4+(row%2)*.5)*17.3+row*39.7)*.055
weather = .025*np.sin(u*math.tau*13)*np.sin(v*math.tau*7)
base = .31 + block + .065*noise + weather
stone = np.stack([base*1.02,base,base*.91],axis=2)
stone *= (.45+.55*stone_height[:,:,None])
wood_height = .5+.16*np.sin(u*math.tau*32+np.sin(v*math.tau*3)*1.7)+.08*noise
wood_base = .14+.075*wood_height+.03*noise
wood = np.stack([wood_base*1.45,wood_base*.97,wood_base*.58],axis=2)
slate_row = np.floor(v*12)
sx=(u*8+(slate_row%2)*.5)%1;sy=(v*12)%1
slate_h=np.clip(np.minimum(np.minimum(sx,1-sx),sy)*30,0,1)*(.7+.2*noise)
slate_b=.11+.045*noise+.025*np.sin(np.floor(u*8+(slate_row%2)*.5)*7+slate_row*13)
slate=np.stack([slate_b*.72,slate_b*.87,slate_b],axis=2)*(.45+.55*slate_h[:,:,None])
def texture(name, pixels, color=True):
    rgba=np.ones((N,N,4),dtype=np.float32);rgba[:,:,:3]=pixels
    image=bpy.data.images.new('Greyhaven '+name,width=N,height=N)
    if not color: image.colorspace_settings.name='Non-Color'
    image.pixels.foreach_set(rgba.ravel());image.filepath_raw=str(OUT/(name+'.png'));image.file_format='PNG';image.save()
    return image

def material(name, pixels, height, roughness, scale):
    m=bpy.data.materials.new('Greyhaven '+name);m.use_nodes=True
    p=next(n for n in m.node_tree.nodes if n.type=='BSDF_PRINCIPLED');p.inputs['Roughness'].default_value=roughness
    im=m.node_tree.nodes.new('ShaderNodeTexImage');im.image=texture(name+'-color',pixels);m.node_tree.links.new(im.outputs['Color'],p.inputs['Base Color'])
    dx=np.roll(height,-1,1)-np.roll(height,1,1);dy=np.roll(height,-1,0)-np.roll(height,1,0)
    normal=np.stack([-dx*2,-dy*2,np.ones_like(dx)],axis=2);normal/=np.linalg.norm(normal,axis=2)[:,:,None];normal=normal*.5+.5
    nm=m.node_tree.nodes.new('ShaderNodeTexImage');nm.image=texture(name+'-normal',normal,False)
    normal_node=m.node_tree.nodes.new('ShaderNodeNormalMap');m.node_tree.links.new(nm.outputs['Color'],normal_node.inputs['Color']);m.node_tree.links.new(normal_node.outputs['Normal'],p.inputs['Normal'])
    m['tile_metres']=scale
    return m
stone_mat=material('weathered-masonry',stone,stone_height,.88,3.2)
wood_mat=material('weathered-oak',wood,wood_height,.82,2)
slate_mat=material('slate',slate,slate_h,.76,3)
def flat(name,color,metal=0):
    m=bpy.data.materials.new('Greyhaven '+name);m.use_nodes=True;p=next(n for n in m.node_tree.nodes if n.type=='BSDF_PRINCIPLED');p.inputs['Base Color'].default_value=(*color,1);p.inputs['Metallic'].default_value=metal;p.inputs['Roughness'].default_value=.65;return m
trim=flat('limestone dressings',(.43,.40,.33));iron=flat('forged iron',(.055,.064,.069),.65);dark=flat('deep window recess',(.018,.025,.026));blue=flat('indigo linen',(.035,.10,.17));gold=flat('ochre heraldry',(.60,.40,.13))

def mesh(name,vs,fs,mat):
    data=bpy.data.meshes.new(name);data.from_pydata([(x,-z,y) for x,y,z in vs],[],fs);data.materials.append(mat);data.update()
    o=bpy.data.objects.new(name,data);collection.objects.link(o);objects.append(o)
    uv=data.uv_layers.new(name='Metre projection');scale=mat.get('tile_metres',2)
    for face in data.polygons:
        axis=max(range(3),key=lambda a:abs(face.normal[a]));axes=[a for a in range(3) if a!=axis]
        for loop in face.loop_indices:
            co=data.vertices[data.loops[loop].vertex_index].co;uv.data[loop].uv=(co[axes[0]]/scale,co[axes[1]]/scale)
    return o
BOX_FACES=[(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)]
# Box order in game coordinates, outward faces after the right-handed basis change.
def box(name,c,h,mat,bevel=0):
    x,y,z=c;a,b,d=h
    vs=[(x+sx*a,y+sy*b,z+sz*d) for sx,sy,sz in [(-1,-1,-1),(1,-1,-1),(1,-1,1),(-1,-1,1),(-1,1,-1),(1,1,-1),(1,1,1),(-1,1,1)]]
    o=mesh(name,vs,BOX_FACES,mat)
    if bevel:
        mod=o.modifiers.new('Worn stone arris','BEVEL');mod.width=bevel;mod.segments=2
    return o

def beam(name,a,b,r,mat):
    a,b=Vector(a),Vector(b);d=(b-a).normalized();ref=Vector((0,1,0)) if abs(d.y)<.9 else Vector((1,0,0));q=d.cross(ref).normalized()*r;t=d.cross(q).normalized()*r
    vs=[tuple(p+q*s+t*v) for p in [a,b] for s,v in [(-1,-1),(1,-1),(1,1),(-1,1)]]
    return mesh(name,vs,BOX_FACES,mat)

def cylinder(name,c,r,h,mat,n=12,tip=None):
    x,y,z=c;top=r if tip is None else tip
    vs=[(x+radius*math.sin(i*math.tau/n),y+dy,z+radius*math.cos(i*math.tau/n)) for dy,radius in [(0,r),(h,top)] for i in range(n)]
    fs=[tuple(range(n-1,-1,-1)),tuple(range(n,2*n))]+[(i,(i+1)%n,(i+1)%n+n,i+n) for i in range(n)]
    return mesh(name,vs,fs,mat)

def solid_record(id,vs,fs):
    triangles=[]
    centre=sum((Vector(v) for v in vs),Vector())/len(vs)
    for face in fs:
        f=list(face)
        a,b,c=(Vector(vs[i]) for i in f[:3])
        if (b-a).cross(c-a).dot((a+b+c)/3-centre)<0:f.reverse()
        for i in range(1,len(f)-1):triangles.append([f[0],f[i],f[i+1]])
    collision.append(dict(id=id,position=[0,0,0],vertices=[[round(v*100) for v in p] for p in vs],triangles=triangles))

def roof(name,x,z,w,d,y,rise,mat,body=None,open_gables=False):
    vs=[(x-w,y,z-d),(x+w,y,z-d),(x+w,y,z+d),(x-w,y,z+d),(x-w,y+rise,z),(x+w,y+rise,z)]
    fs=[(0,3,2,1),(0,1,5,4),(3,4,5,2),(0,4,3),(1,2,5)]
    mesh(name,vs,fs[:3] if open_gables else fs,mat)
    if body is not None:solid_record(body,vs,fs)

# Keep: dressed stone hall, four engaged round turrets, slate roof and an inset portal.
box('Keep ashlar hall',(-17,5,1),(9,5,6),stone_mat,.07)
box('Keep battered plinth',(-17,.3,1),(9.05,.3,6.05),trim,.05)
for y in [3.4,7.0,9.85]: box('Keep string course',(-17,y,1),(9.035,.105,6.035),trim,.025)
roof('Keep slate hall roof',-17,1,9.2,6.2,10.05,3.1,slate_mat,65000)
for i,(x,z) in enumerate([(-24.35,-3.35),(-9.65,-3.35),(-24.35,5.35),(-9.65,5.35)]):
    cylinder('Keep round tower',(x,0,z),1.55,12.0,stone_mat,16)
    cylinder('Keep tower cornice',(x,11.6,z),1.6,.4,trim,16)
    cylinder('Keep conical slate cap',(x,12,z),1.6,3.0,slate_mat,16,0)
    # Simple closed sixteen-sided solid through the tower and its cap.
    vs=[(x+1.55*math.sin(j*math.tau/16),h,z+1.55*math.cos(j*math.tau/16)) for h in [0,12] for j in range(16)]+[(x,15,z)]
    fs=[tuple(range(15,-1,-1))]+[(j,(j+1)%16,(j+1)%16+16,j+16) for j in range(16)]+[(j+16,(j+1)%16+16,32) for j in range(16)]
    solid_record(65001+i,vs,fs)
    for h in [5,9.4]:
        box('Tower arrow slit',(x,h,z+1.553),(.11,.6,.012),dark)
        box('Tower slit sill',(x,h-.65,z+1.57),(.22,.08,.09),trim,.02)
for x in [-22,-19,-15,-12]:
    for y in [4.7,8.15]:
        box('Keep window recess',(x,y,7.012),(.35,.75,.014),dark)
        for dx in [-.43,.43]:box('Keep window jamb',(x+dx,y,7.045),(.09,.88,.095),trim,.025)
        for dy in [-.84,.84]:box('Keep window lintel',(x,y+dy,7.05),(.48,.095,.10),trim,.025)
        box('Keep window mullion',(x,y,7.08),(.045,.72,.045),iron)
# Portal is closed, as the authoritative keep remains a solid building.
box('Recessed oak double door',(-17,1.55,7.018),(1.1,1.55,.02),wood_mat)
for x in [-18.23,-15.77]:box('Portal dressed jamb',(x,1.55,7.12),(.14,1.55,.17),trim,.03)
for j in range(9):
    a=j*math.pi/8
    o=box('Portal voussoir',(-17+1.22*math.cos(a),3.10+1.22*math.sin(a),7.12),(.22,.17,.18),trim,.025);o.rotation_euler[1]=0 # stones are individual worn dressings
for x in [-17.55,-16.45]:
    for y in [.7,2.2]:box('Door iron strap',(x,y,7.05),(.5,.07,.04),iron)
for x in [-21.5,-12.5]:
    box('Indigo keep banner',(x,5.6,7.15),(.48,1.45,.025),blue)
    box('Greyhaven banner device',(x,5.8,7.185),(.10,.55,.015),gold)
for x in [-23,-20,-14,-11]:
    box('South wall buttress',(x,2.8,7.12),(.26,2.8,.18),stone_mat,.045)


# A broken roof silhouette and deeper facade details give the small frontier keep a lived-in
# shape. Dormers and stacks have coarse closed collision; trim stays within those surfaces.
for i,x in enumerate([-20.5,-13.5]):
    front,back=5.95,3.85
    section=[(x-.85,10.45),(x+.85,10.45),(x+.85,12.0),(x,12.85),(x-.85,12.0)]
    vs=[(px,py,z) for z in [back,front] for px,py in section]
    fs=[(4,3,2,1,0),(5,6,7,8,9)]+[(j,(j+1)%5,(j+1)%5+5,j+5) for j in range(5)]
    solid_record(65067+i,vs,fs)
    mesh('Keep stone dormer',vs,fs[:4]+fs[6:],stone_mat)
    mesh('Keep dormer slate slopes',vs,fs[4:6],slate_mat)
    box('Dormer dark glass',(x,11.63,front+.013),(.50,.43,.018),dark)
    for dx in [-.56,.56]:box('Dormer stone jamb',(x+dx,11.63,front+.06),(.07,.51,.09),trim,.02)
    for y in [11.12,12.14]:box('Dormer stone sill',(x,y,front+.08),(.63,.07,.12),trim,.02)
    box('Dormer oak mullion',(x,11.64,front+.055),(.035,.43,.04),wood_mat)
    for side in [-1,1]:beam('Dormer gable verge',(x,12.9,front+.06),(x+side*.94,11.96,front+.06),.075,trim)
for i,(x,z) in enumerate([(-22.0,-.6),(-12.0,1.8)]):
    box('Keep masonry chimney',(x,13.1,z),(.48,1.4,.54),stone_mat,.035)
    box('Keep chimney cap',(x,14.45,z),(.60,.14,.66),trim,.025)
    box('Keep soot mouth',(x,14.60,z),(.36,.015,.42),dark)
    vs=[(x+sx*.60,y,z+sz*.66) for y in [11.7,14.60] for sx,sz in [(-1,-1),(1,-1),(1,1),(-1,1)]]
    solid_record(65065+i,vs,BOX_FACES)
for z in [-5.2,7.2]:
    beam('Keep roof eave fascia',(-26.25,10.08,z),(-7.75,10.08,z),.095,wood_mat)
beam('Keep ridge capping',(-26.25,13.17,1),(-7.75,13.17,1),.12,slate_mat)
for x in [-22,-19,-15,-12]:
    for y in [4.7,8.15]:
        box('Keep window crossbar',(x,y,7.105),(.35,.035,.045),iron)
        for side in [-1,1]:
            box('Keep open oak shutter',(x+side*.59,y,7.09),(.14,.72,.055),wood_mat)
            for dy in [-.48,.48]:box('Keep shutter hinge',(x+side*.59,y+dy,7.155),(.15,.035,.018),iron)


# The approach from the north and the side lanes sees finished elevations too.
for side in [-1,1]:
    face_x=-17+side*9.015
    for z in [-2.5,1,4.5]:
        for y in [4.7,8.15]:
            box('Keep side window recess',(face_x,y,z),(.015,.75,.35),dark)
            for dz in [-.43,.43]:box('Keep side window jamb',(face_x+side*.04,y,z+dz),(.095,.88,.09),trim,.025)
            for dy in [-.84,.84]:box('Keep side head and sill',(face_x+side*.05,y+dy,z),(.10,.095,.48),trim,.025)
            box('Keep side mullion',(face_x+side*.08,y,z),(.045,.72,.045),iron)
            box('Keep side transom',(face_x+side*.08,y,z),(.045,.035,.35),iron)
    for z in [-3.4,5.4]:box('Keep return buttress',(face_x,2.8,z),(.18,2.8,.26),stone_mat,.045)
for x in [-22,-19,-15,-12]:
    for y in [4.7,8.15]:
        box('Keep north window recess',(x,y,-5.015),(.35,.75,.015),dark)
        for dx in [-.43,.43]:box('Keep north window jamb',(x+dx,y,-5.055),(.09,.88,.095),trim,.025)
        for dy in [-.84,.84]:box('Keep north head and sill',(x,y+dy,-5.065),(.48,.095,.10),trim,.025)
        box('Keep north mullion',(x,y,-5.095),(.045,.72,.045),iron)


# The square's well uses a stone ring and a working-looking timber winding frame.
# Its simplified body is closed (the well cannot be entered); the visible shaft is inset.
x,z=11,30;n=16
vs=[(x+r*math.sin(j*math.tau/n),y,z+r*math.cos(j*math.tau/n))
    for y,r in [(0,1),(.98,1),(.98,.68),(.35,.68)] for j in range(n)]
fs=[(row*n+j,row*n+(j+1)%n,(row+1)*n+(j+1)%n,(row+1)*n+j)
    for row in range(3) for j in range(n)]
mesh('Well weathered stone lining',vs,fs,stone_mat)
cylinder('Well dark inset shaft',(x,.34,z),.68,.02,dark,16)
body=[(x+math.sin(j*math.tau/n),y,z+math.cos(j*math.tau/n)) for y in [0,1] for j in range(n)]
body_faces=[tuple(range(n-1,-1,-1)),tuple(range(n,2*n))]+[(j,(j+1)%n,(j+1)%n+n,j+n) for j in range(n)]
solid_record(107,body,body_faces)
for side in [-1,1]:
    box('Well oak upright',(x+side*.84,1.8,z),(.105,1.0,.105),wood_mat,.015)
    for y in [1.02,2.3]:box('Well post iron collar',(x+side*.84,y,z),(.115,.055,.115),iron)
    beam('Well canopy knee brace',(x+side*.83,2.05,z),(x+side*.35,2.76,z),.06,wood_mat)
beam('Well winding beam',(x-1.04,1.88,z),(x+1.04,1.88,z),.10,wood_mat)
beam('Well crank arm',(x+1.08,1.88,z),(x+1.08,1.56,z),.055,iron)
beam('Well crank handgrip',(x+1.08,1.56,z),(x+1.28,1.56,z),.055,wood_mat)
rope_mat=flat('well hemp',(.30,.24,.14))
cylinder('Well hanging rope',(x,.62,z),.025,1.27,rope_mat,7)
roof('Well slate canopy',x,z,1.25,.95,2.78,.62,slate_mat,65069)
for side in [-1,1]:beam('Well canopy fascia',(x-1.28,2.77,z+side*.95),(x+1.28,2.77,z+side*.95),.06,wood_mat)

# The eight old straight palisades become one continuous, faceted perimeter with four gates.
PATHS={110:[(-3,-10),(-27,-10),(-31,-6)],111:[(3,-10),(27,-10),(31,-6)],112:[(-32,40),(-28,50),(-6.5,50)],113:[(3,50),(28,50),(33,42)],114:[(-31,-6),(-34,5),(-33,13),(-32,17)],115:[(-32,23),(-34,30),(-32,40)],116:[(31,-6),(34,4),(34,13),(32,17)],117:[(32,23),(34,32),(33,42)]}
extra_wall_id=65050
for id,points in PATHS.items():
    wall_vs,wall_fs=[],[]
    for segment,(a,b) in enumerate(zip(points,points[1:])):
        ax,az=a;bx,bz=b;dx,dz=bx-ax,bz-az;length=math.hypot(dx,dz);nx,nz=-dz/length*.30,dx/length*.30
        vs=[(x+nx*s,y,z+nz*s) for y in [0,3] for x,z,s in [(ax,az,-1),(bx,bz,-1),(bx,bz,1),(ax,az,1)]]
        solid_record(id if segment==0 else extra_wall_id,vs,BOX_FACES)
        if segment: extra_wall_id+=1
        count=math.ceil(length/.43)
        for j in range(count):
            t=(j+.5)/count;x=ax+dx*t;z=az+dz*t;h=2.95+.17*math.sin(j*17.3+id)
            cylinder('Split oak palisade',(x,-.12,z),.235,h,wood_mat,7)
            cylinder('Palisade sharpened crown',(x,h-.12,z),.235,.45,wood_mat,7,0)
        for h in [.8,2.25]:beam('Palisade rail',(ax,h,az),(bx,h,bz),.10,wood_mat)
        for j in range(1,math.ceil(length/3)):
            t=j*3/length
            beam('Palisade inside brace',(ax+dx*t+nx*4,0,az+dz*t+nz*4),(ax+dx*t,2.1,az+dz*t),.10,wood_mat)


# Four timber watchtowers, matching simple solid support bodies, with covered lookout decks.
for i,(x,z) in enumerate([(-27,-7),(27,-7),(-30,40),(31,40)]):
    box('Watchtower stone footing',(x,.32,z),(1.4,.32,1.4),stone_mat,.05)
    box('Watchtower timber base',(x,2.65,z),(1.35,2.65,1.35),wood_mat)
    box('Watchtower lookout deck',(x,5.35,z),(1.55,.15,1.55),wood_mat)
    for sx in [-1,1]:
        for sz in [-1,1]:
            box('Watchtower upright',(x+sx*1.3,3.65,z+sz*1.3),(.15,3.65,.15),wood_mat)
    for s in [-1,1]:
        beam('Tower diagonal brace',(x-1.25,.5,z+s*1.37),(x+1.25,4.9,z+s*1.37),.12,wood_mat)
        for h in [5.8,6.35]:
            beam('Lookout rail',(x-1.35,h,z+s*1.35),(x+1.35,h,z+s*1.35),.085,wood_mat)
            beam('Lookout side rail',(x+s*1.35,h,z-1.35),(x+s*1.35,h,z+1.35),.085,wood_mat)
    roof('Watchtower slate cover',x,z,1.7,1.7,7.1,1.6,slate_mat,65010+i)
    box('Watchtower banner',(x,4.0,z+1.375),(.42,.7,.025),blue)
# Gate frames retain the exact four road openings; open leaves fold along the wall.
for i,(x,z,turn) in enumerate([(0,-10,0),(0,50,0),(-32,20,1),(32,20,1)]):
    def gatebox(name,c,h,mat):
        dx,y,dz=c
        return box(name,(x+(dz if turn else dx),y,z+(-dx if turn else dz)),(h[2],h[1],h[0]) if turn else h,mat,.025)
    gatebox('Gate lintel',(0,4.15,0),(3.15,.22,.4),wood_mat)
    roof_start=len(objects)
    roof('Gate slate canopy',0,0,3.3,.85,4.37,.85,slate_mat)
    canopy=objects[-1]
    for v in canopy.data.vertices:
        gx,gz=v.co.x,-v.co.y
        v.co.x=x+(gz if turn else gx);v.co.y=-(z+(-gx if turn else gz))
    canopy.data.update()
    for side in [-1,1]:
        gatebox('Iron-bound gate pillar',(side*2.6,2,0),(.4,2,.4),wood_mat)
        for y in [.5,2.8]:gatebox('Gate pillar iron collar',(side*2.6,y,0),(.425,.10,.425),iron)
        gatebox('Folded gate leaf',(side*3.6,1.35,.4),(.85,1.35,.12),wood_mat)
        for y in [.4,2.25]:gatebox('Gate leaf strap',(side*3.6,y,.54),(.8,.07,.04),iron)
    gatebox('Gate heraldic board',(0,4.15,.46),(.5,.45,.06),blue)
    gatebox('Gate board device',(0,4.15,.54),(.12,.3,.025),gold)
    # Header blocks jumping/climbing into the lintel while keeping 3.9 m head clearance.
    hx,hz=(.4,3.15) if turn else (3.15,.4)
    vs=[(x+sx*hx,3.93+sy*.44,z+sz*hz) for sx,sy,sz in [(-1,0,-1),(1,0,-1),(1,0,1),(-1,0,1),(-1,1,-1),(1,1,-1),(1,1,1),(-1,1,1)]]
    solid_record(65020+i,vs,BOX_FACES)
# Millbrook's three existing closed buildings: local geometry is placed on their unchanged
# content footprints. The windmill keeps the game's shared animated sail assembly.
red_mat=material('barn-oxblood',np.clip(wood*np.array([1.25,.72,.56]),0,1),wood_height,.9,2)
pv,pu=np.mgrid[0:N,0:N]/N
plaster_h=.5+.055*noise+.025*np.sin(pu*math.tau*3)*np.sin(pv*math.tau*5)
plaster_base=.48+.055*noise+.025*np.sin(pu*math.tau*3)*np.sin(pv*math.tau*2)
plaster_mat=material('lime-plaster',np.stack([plaster_base,plaster_base*.94,plaster_base*.79],axis=2),plaster_h,.96,2.8)
warm_window=flat('farm warm glass',(.20,.12,.052))

def farm_place(start,solid_start,x,z,quarter,base=11):
    # Positive game yaw turns +Z toward +X, as the shared procedural building placement does.
    a=quarter*math.pi/2;c,s=math.cos(a),math.sin(a)
    def world(gx,gy,gz):return (x+c*gx+s*gz,base+gy,z-s*gx+c*gz)
    for obj in objects[start:]:
        for v in obj.data.vertices:
            gx,gy,gz=v.co.x,v.co.z,-v.co.y;px,py,pz=world(gx,gy,gz);v.co=(px,-pz,py)
        obj.data.update()
    for record in collision[solid_start:]:
        record['vertices']=[[round(v*100) for v in world(*(q/100 for q in p))] for p in record['vertices']]

def gable_panel(name,w,d,base,rise,mat):
    for sign in [-1,1]:mesh(name,[(-w,base,sign*d),(w,base,sign*d),(0,base+rise,sign*d)],[(0,1,2)],mat)

def farm_window(x,y,z,side=False,outward=1):
    # Recess and shallow mullions sit against the solid wall; no extra movement blockers.
    def detail(name,c,h,mat):
        xx,yy,zz=c
        return box(name,(x+zz*outward if side else x+xx,y+yy,z+xx if side else z+zz*outward),(h[2],h[1],h[0]) if side else h,mat,.015)
    detail('Farm window recess',(0,0,0),(.48,.64,.025),dark)
    detail('Farm amber glass',(0,0,.03),(.37,.53,.012),warm_window)
    for dx in [-.45,.45]:detail('Farm window jamb',(dx,0,.055),(.065,.66,.06),wood_mat)
    for dy in [-.61,.61]:detail('Farm window head and sill',(0,dy,.065),(.52,.065,.08),wood_mat)
    detail('Farm window mullion',(0,0,.07),(.035,.55,.045),wood_mat)
    detail('Farm window transom',(0,0,.07),(.39,.035,.045),wood_mat)

# Barn: oxblood plank walls over a stone sill, heavy bracing, a steep roof and closed loft.
start,solid_start=len(objects),len(collision)
box('Millbrook barn masonry sill',(0,.30,0),(6,.30,8),stone_mat,.05)
box('Millbrook barn plank hall',(0,3.8,0),(6,3.2,8),red_mat,.025)
# Long ridge runs along local Z; rotate the existing roof primitive through a quarter turn.
roof_start=len(objects);roof('Millbrook barn slate roof',0,0,8.25,6.25,7,4.2,slate_mat,65060,open_gables=True)
for obj in objects[roof_start:]:
    for v in obj.data.vertices:v.co.x,v.co.y=-v.co.y,v.co.x
    obj.data.update()
for record in collision[solid_start:]:
    record['vertices']=[[p[2],p[1],-p[0]] for p in record['vertices']]
gable_panel('Millbrook barn gable',6,8,7,4.2,red_mat)
for side in [-1,1]:
    for z in [-7.8,-3.9,0,3.9,7.8]:
        box('Barn side post',(side*6.025,3.8,z),(.10,3.2,.14),wood_mat)
    for y in [.7,4.4,6.9]:box('Barn long rail',(side*6.04,y,0),(.11,.11,7.95),wood_mat)
    for z in [-5.9,2.0]:
        beam('Barn side diagonal',(side*6.07,.8,z-1.75),(side*6.07,4.3,z+1.75),.105,wood_mat)
for side in [-1,1]:
    beam('Barn gable rake',(-6,7,side*8.04),(0,11.2,side*8.04),.11,wood_mat)
    beam('Barn gable rake',(0,11.2,side*8.04),(6,7,side*8.04),.11,wood_mat)
    beam('Barn gable king post',(0,7,side*8.05),(0,11.2,side*8.05),.10,wood_mat)
    box('Barn closed loft hatch',(0,8.1,side*8.08),(.65,.9,.06),wood_mat)
box('Barn double oak door',(0,2.2,8.065),(1.65,2.2,.065),wood_mat)
for x in [-1.7,1.7]:box('Barn door frame',(x,2.25,8.12),(.14,2.25,.13),wood_mat)
box('Barn door lintel',(0,4.5,8.12),(1.85,.16,.14),wood_mat)
for side in [-1,1]:
    beam('Barn door diagonal',(side*.12,.25,8.15),(side*1.55,4.1,8.15),.085,wood_mat)
    for y in [.7,3.6]:box('Barn forged hinge',(side*.85,y,8.17),(.65,.055,.025),iron)
farm_place(start,solid_start,80,44,2)

# Farmhouse: pale limewash, exposed framing, slate roof, chimney and closed oak front door.
start,solid_start=len(objects),len(collision)
box('Millbrook farmhouse footing',(0,.35,0),(4,.35,3.5),stone_mat,.06)
box('Millbrook farmhouse limewash',(0,2.85,0),(4,2.15,3.5),plaster_mat,.035)
roof('Millbrook farmhouse slate roof',0,0,4.25,3.75,5,2.4,slate_mat,65061,open_gables=True)
# Ridge along X: gable infill on the two X ends.
for side in [-1,1]:
    mesh('Farmhouse plaster gable',[(side*4,5,-3.5),(side*4,5,3.5),(side*4,7.4,0)],[(0,1,2)],plaster_mat)
    for z in [-3.5,3.5]:beam('Farmhouse gable rake',(side*4.04,5,z),(side*4.04,7.4,0),.09,wood_mat)
    for z in [-3.25,0,3.25]:box('Farmhouse side framing',(side*4.02,2.8,z),(.10,2.15,.11),wood_mat)
    for x in [-3.85,-1.75,1.75,3.85]:box('Farmhouse front framing',(x,2.85,side*3.52),(.10,2.15,.10),wood_mat)
    for y in [.8,2.8,4.9]:
        box('Farmhouse horizontal front beam',(0,y,side*3.54),(4,.095,.11),wood_mat)
        box('Farmhouse horizontal side beam',(side*4.04,y,0),(.11,.095,3.5),wood_mat)
    for x in [-2.7,2.7]:farm_window(x,3.8,side*3.57,outward=side)
for side in [-1,1]:farm_window(side*4.08,3.7,0,True,side)
box('Farmhouse oak door',(0,1.3,3.565),(.65,1.3,.065),wood_mat)
for x in [-.73,.73]:box('Farmhouse portal post',(x,1.4,3.62),(.10,1.4,.12),wood_mat)
box('Farmhouse portal lintel',(0,2.8,3.62),(.83,.11,.14),wood_mat)
box('Farmhouse latch',(.45,1.3,3.66),(.07,.035,.03),iron)
box('Farmhouse chimney',(-2.5,6.3,-1.3),(.40,1.3,.40),stone_mat,.04)
box('Farmhouse chimney crown',(-2.5,7.6,-1.3),(.47,.10,.47),trim,.025)
vs=[(-2.5+sx*.47,y,-1.3+sz*.47) for y in [5,7.7] for sx,sz in [(-1,-1),(1,-1),(1,1),(-1,1)]]
solid_record(65063,vs,BOX_FACES)
farm_place(start,solid_start,62,46.5,2)

# Tapered mill tower: exact closed 20-sided collision instead of the old invisible square
# corners. Its animated sails retain their existing hub at local (0,8.9,2.85).
start,solid_start=len(objects),len(collision)
n=20;vs=[(r*math.sin(j*math.tau/n),y,r*math.cos(j*math.tau/n)) for y,r in [(0,2.5),(8,1.95)] for j in range(n)]
fs=[tuple(range(n-1,-1,-1)),tuple(range(n,2*n))]+[(j,(j+1)%n,(j+1)%n+n,j+n) for j in range(n)]
mesh('Millbrook tapering stone windmill',vs,fs,stone_mat);solid_record(202,vs,fs)
for y,r in [(.15,2.50),(3.0,2.30),(6.5,2.06)]:cylinder('Mill stone belt',(0,y,0),r,.13,trim,20,r-.012)
cylinder('Mill conical slate cap',(0,8,0),2.24,2.6,slate_mat,20,0)
vs=[(2.24*math.sin(j*math.tau/n),8,2.24*math.cos(j*math.tau/n)) for j in range(n)]+[(0,10.6,0)]
solid_record(65062,vs,[tuple(range(n-1,-1,-1))]+[(j,(j+1)%n,n) for j in range(n)])
box('Mill closed oak door',(0,1.1,2.43),(.47,1.1,.055),wood_mat)
for x in [-.54,.54]:box('Mill door stone jamb',(x,1.18,2.44),(.09,1.18,.09),trim,.025)
box('Mill portal lintel',(0,2.36,2.44),(.63,.10,.13),trim,.035)
farm_window(0,4.9,2.18)
box('Mill timber axle housing',(0,8.9,2.35),(.22,.22,.50),wood_mat)
vs=[(sx*.22,8.68+sy*.44,2.35+sz*.50) for sx,sy,sz in [(-1,0,-1),(1,0,-1),(1,0,1),(-1,0,1),(-1,1,-1),(1,1,-1),(1,1,1),(-1,1,1)]]
solid_record(65064,vs,BOX_FACES)
farm_place(start,solid_start,88,21,2)

# The Hollow's existing three closed tents and four supply crates. Cloth folds are visual;
# each tent's simplified filled prism supplies stable-ID collision independently of them.
cv,cu=np.mgrid[0:N,0:N]/N
weave=.5+.018*np.sin(cu*math.tau*128)*np.sin(cv*math.tau*128)+.02*noise
cloth_base=.43+.07*noise+.035*np.sin(cu*math.tau*3)*np.cos(cv*math.tau*2)
cloth=material('weathered-canvas',np.stack([cloth_base,cloth_base*.90,cloth_base*.72],axis=2),weave,.97,1.5)
seam=flat('canvas binding',(.28,.23,.15))
for id,x,z,base,quarter in [(420,-15.5,-94.5,11,1),(421,12.5,-97.5,11,3),(422,-13.5,-84.5,11.36,1)]:
    start,solid_start=len(objects),len(collision)
    section=[(-1.5,0),(1.5,0),(1.5,.35),(0,2.2),(-1.5,.35)]
    vs=[(xx,yy,zz) for zz in [-1.5,1.5] for xx,yy in section]
    fs=[(4,3,2,1,0),(5,6,7,8,9)]+[(j,(j+1)%5,(j+1)%5+5,j+5) for j in range(5)]
    solid_record(id,vs,fs)
    # Low canvas skirts and closed end flaps follow the same contact footprint.
    mesh('Hollow tent canvas skirt',vs,[fs[2],fs[3],fs[-1]],cloth)
    mesh('Hollow tent closed flaps',vs,fs[:2],cloth)
    for side in [-1,1]:
        vs=[];rows,cols=8,7
        for r in range(rows+1):
            zz=-1.5+3*r/rows
            for c in range(cols+1):
                t=c/cols;xx=side*1.5*t
                fold=.025*math.sin(t*math.pi)*math.sin(zz*math.pi*3)**2
                yy=2.2-1.85*t+fold
                vs.append((xx,yy,zz))
        fs=[]
        for r in range(rows):
            for c in range(cols):
                a=r*(cols+1)+c;face=(a,a+1,a+cols+2,a+cols+1);fs.append(face[::-1] if side>0 else face)
        mesh('Hollow tent draped canvas',vs,fs,cloth)
    for zz in [-1.48,1.48]:
        beam('Hollow tent ridge pole',(0,0,zz),(0,2.2,zz),.035,wood_mat)
        for side in [-1,1]:beam('Hollow tent bound seam',(side*1.5,.35,zz),(0,2.2,zz),.018,seam)
    beam('Hollow tent closed flap seam',(0,.04,1.515),(0,2.17,1.515),.012,seam)
    beam('Hollow tent ridge binding',(0,2.2,-1.5),(0,2.2,1.5),.018,seam)
    farm_place(start,solid_start,x,z,quarter,base)

for x,z in [(5.5,-104.5),(6.7,-104.5),(6.1,-103.3),(-6.5,-102.5)]:
    start,solid_start=len(objects),len(collision)
    box('Hollow supply crate boards',(0,.5,0),(.475,.5,.475),wood_mat,.018)
    for side in [-1,1]:
        for axis in [-1,1]:
            box('Hollow crate corner',(side*.455,.5,axis*.455),(.045,.5,.045),wood_mat,.01)
        for y in [.055,.945]:
            box('Hollow crate front rim',(0,y,side*.455),(.5,.055,.045),wood_mat)
            box('Hollow crate side rim',(side*.455,y,0),(.045,.055,.5),wood_mat)
        beam('Hollow crate diagonal',(-.40,.14,side*.49),(.40,.86,side*.49),.028,wood_mat)
        for xx in [-.43,.43]:
            for yy in [.14,.86]:box('Hollow crate iron nail',(xx,yy,side*.495),(.018,.018,.005),iron)
    farm_place(start,solid_start,x,z,0)

# The deeper public-cave threshold stays sealed. Dress its unchanged solid footprint with
# closed oak boards and heavy braces, rather than suggesting an open continuation.
box('Greyfang sealed timber threshold',(79.6,12.75,-138),(.3,1.75,2),wood_mat,.025)
for z in [-139.85,-136.15]:box('Greyfang threshold upright',(79.325,12.75,z),(.025,1.75,.12),wood_mat)
for y in [11.15,14.35]:box('Greyfang threshold header',(79.325,y,-138),(.025,.13,1.95),wood_mat)
beam('Greyfang threshold diagonal',(79.31,11.3,-139.7),(79.31,14.2,-136.3),.07,wood_mat)
for z in [-139.6,-136.4]:
    for y in [11.4,14.1]:box('Greyfang threshold iron strap',(79.29,y,z),(.015,.06,.24),iron)

# The brook crossing dresses the existing deck, rails and approach ramp bodies. The
# shared procedural bridge remains the fallback until the settlement art is ready.
def bridge_grain(obj,axis):
    along=Vector((axis[0],-axis[2],axis[1])).normalized();data=obj.data
    uv=data.uv_layers.active;tile=wood_mat['tile_metres']
    for face in data.polygons:
        across=along.cross(face.normal)
        if across.length<.001:continue
        across.normalize()
        for loop in face.loop_indices:
            co=data.vertices[data.loops[loop].vertex_index].co
            uv.data[loop].uv=(co.dot(across)/tile,co.dot(along)/tile)

for i in range(40):
    z=30.05+i*.1
    plank=box('Brook weathered deck plank',(0,.065,z),(1.5,.055,.046),wood_mat,.006)
    bridge_grain(plank,(1,0,0))
    for x in [-1.27,1.27]:cylinder('Brook deck iron peg',(x,.121,z),.018,.006,iron,6)
for x in [-1.8,1.8]:
    for z in [30,31,32,33,34]:
        post=box('Brook fitted oak post',(x,.65,z),(.10,.55,.10),wood_mat,.015)
        bridge_grain(post,(0,1,0))
        for y in [.25,1.08]:box('Brook post iron collar',(x,y,z),(.105,.035,.105),iron)
        for y in [.50,1.05]:box('Brook rail iron pin',(x+math.copysign(.085,x),y,z),(.012,.025,.025),iron)
    for y in [.50,1.05]:
        rail=box('Brook oak handrail',(x,y,32),(.07,.06,2.3),wood_mat,.012)
        bridge_grain(rail,(0,0,1))
    for z in [30,31,32,33]:
        a=(x,.30,z+.08);b=(x,1.02,z+.92)
        brace=beam('Brook rail diagonal',a,b,.035,wood_mat)
        bridge_grain(brace,tuple(b[i]-a[i] for i in range(3)))
for x in [-1.2,1.2]:
    stringer=box('Brook timber stringer',(x,-.12,32),(.10,.08,2.05),wood_mat)
    bridge_grain(stringer,(0,0,1))
for south in [False,True]:
    vs=[(-1.5,0,28),(1.5,0,28),(1.5,0,30),(-1.5,0,30),(-1.5,.12,30),(1.5,.12,30)]
    if south:vs=[(x,y,64-z) for x,y,z in vs]
    ramp=mesh('Brook approach frame',vs,[(0,3,2,1),(3,4,5,2),(0,4,3),(1,2,5)],wood_mat)
    bridge_grain(ramp,(0,0,1))
    for i in range(20):
        a,b=28+i*.1+.004,28+(i+1)*.1-.004
        board=[(x,.06*(z-28)-drop,64-z if south else z)
            for drop in [.025,0] for x,z in [(-1.5,a),(1.5,a),(1.5,b),(-1.5,b)]]
        plank=mesh('Brook sloped approach plank',board,BOX_FACES,wood_mat)
        bridge_grain(plank,(1,0,0))
    box('Brook bedded stone sill',(0,-.12,36 if south else 28),(1.52,.12,.12),stone_mat,.02)

# Passable ground cover: a small reusable fern built from curved fronds and folded
# leaflets. Opaque two-sided leaf geometry needs no alpha texture or new renderer feature.
fern_mats=[flat('fern shadow leaf',(.055,.13,.028)),flat('fern green leaf',(.085,.19,.04)),
    flat('fern young leaf',(.12,.23,.05))]
for mat in fern_mats:
    next(n for n in mat.node_tree.nodes if n.type=='BSDF_PRINCIPLED').inputs['Roughness'].default_value=.88

def fern(x,y,z,scale,turn,variant):
    leaves=[([],[]) for _ in fern_mats];stem_v,stem_f=[],[]
    for frond in range(7):
        a=turn+frond*math.tau/7+.08*math.sin(frond*7+variant)
        direction=Vector((math.cos(a),0,math.sin(a)))
        across=Vector((-math.sin(a),0,math.cos(a)))
        length=scale*(.80+.20*math.sin(frond*2.3+variant))
        height=scale*(.66+.13*math.sin(frond*3.1+variant))
        def point(t):
            return Vector((x,y+height*math.sin(math.pi*t*.9)-.12*(1-t),z))+direction*(length*t)
        first=len(stem_v)
        for j in range(11):
            p=point(j/10)
            stem_v.extend([tuple(p-across*.009*scale),tuple(p+across*.009*scale)])
        stem_f.extend([(first+2*j,first+2*j+1,first+2*j+3,first+2*j+2) for j in range(10)])
        for pair in range(9):
            t=.13+pair*.09;p=point(t)
            blade=.31*scale*(1-t)**.65;width=.045*scale*(1-t)**.4
            for side in [-1,1]:
                end=p+across*(side*blade)+direction*(.09*scale)
                middle=(p+end)/2
                vs,fs=leaves[(frond+pair+variant)%len(fern_mats)];first=len(vs)
                vs.extend([tuple(p),tuple(middle-direction*width+Vector((0,-.012*scale,0))),
                    tuple(middle+Vector((0,.035*scale,0))),
                    tuple(middle+direction*width+Vector((0,-.012*scale,0))),tuple(end)])
                fs.extend([tuple(first+i for i in face) for face in [(0,1,2),(1,4,2),(4,3,2),(3,0,2)]])
    mesh('Greyhaven fern winding stems',stem_v,stem_f,fern_mats[0])
    for mat,(vs,fs) in zip(fern_mats,leaves):mesh('Greyhaven fern folded leaflets',vs,fs,mat)

# Explicit authored patches on the level ground outside the knolls, clear of paths and
# buildings. Roots begin below the grass surface; foliage deliberately has no solid body.
for patch,(x,z) in enumerate([(-32,-37),(-17,-31),(-7,-32),(-28,-50)]):
    for plant,(dx,dz,size) in enumerate([(0,0,1.25),(-.7,.45,.9),(.65,.55,1.05),(-.35,-.65,.95),(.65,-.55,.8)]):
        fern(x+dx,0,z+dz,size,patch*.8+plant*1.7,patch*5+plant)

# Apply bevels and join by material for a compact, standard static glTF.
bpy.ops.object.select_all(action='DESELECT')
for o in objects:o.select_set(True)
bpy.context.view_layer.objects.active=objects[0]
bpy.ops.object.convert(target='MESH')
bpy.ops.object.join()
o=bpy.context.object;o.name='Greyhaven keep and timber defences'
# Recalculate outside normals explicitly after the Y-up to Z-up transformation.
import bmesh
bm=bmesh.new();bm.from_mesh(o.data);bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces));bm.to_mesh(o.data);bm.free()
options=bpy.ops.export_scene.gltf.get_rna_type().properties['export_format'].enum_items
formats=[v.identifier for v in options]
if not formats:
    try: bpy.ops.export_scene.gltf(export_format='DISCOVER_FORMATS')
    except TypeError as error:
        import re
        formats=re.findall(r"'([^']+)'",str(error))
fmt=next(v for v in formats if 'SEPARATE' in v)
bpy.ops.export_scene.gltf(filepath=str(OUT/'settlement.gltf'),export_format=fmt,use_selection=True,use_active_scene=True,export_yup=True,export_animations=False,export_extras=False)
# One declaration for the existing static bundle loaders; no custom importer.
gltf=json.loads((OUT/'settlement.gltf').read_text());uris=[v['uri'] for key in ['buffers','images'] for v in gltf.get(key,[]) if 'uri' in v]
resources=[dict(path=p,byteLength=(OUT/p).stat().st_size,sha256=hashlib.sha256((OUT/p).read_bytes()).hexdigest()) for p in sorted(set(uris))]
manifest=dict(assets=[dict(key='greyhaven.fort.settlement',variant='default',path='settlement.gltf',resources=resources)],authoring='content/greyhaven/authoring/fortifications.py',license='MIT',coordinateSystem='right-handed-y-up',unit='metre')
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
# Three coarse, fractured crags break up the basin's formerly uniform cliff lip.
# The shared rock presenter draws their exact collision triangles with its existing PBR rock.
for k,(x,z) in enumerate([(-47,43),(-46,62),(47,56)]):
    n=7;vs=[]
    for y,r in [(0,2.4),(8.8,2.9),(13.2,1.8)]:
        for j in range(n):
            a=j*math.tau/n;w=1+.12*math.sin(j*17+k*3)
            vs.append((x+r*w*math.sin(a),y+.3*math.sin(j*7+k),z+r*w*math.cos(a)))
    fs=[tuple(range(n-1,-1,-1)),tuple(range(2*n,3*n))]
    fs += [(r*n+j,r*n+(j+1)%n,(r+1)*n+(j+1)%n,(r+1)*n+j) for r in range(2) for j in range(n)]
    solid_record(65030+k,vs,fs)
# Low shoreline clusters: game collision and shared rock skins use these same coarse
# facets. They break the water outline without new render assets or blocking the dock.
shore_preview=flat('Stillwater shore rock preview',(.29,.31,.23))
for cluster,(x,z,turn) in enumerate([(39,-60,.4),(38,-65,1.1),(47,-70.5,.2),
    (53,-71.5,1.4),(69,-69.5,.8),(39,-41,1.2),(55,-39.5,.6),(69,-40.5,1.5)]):
    for piece,(dx,dz,rx,rz,h) in enumerate([(0,0,2.1,1.35,2.25),(1.8,.7,1.1,.85,1.45),(-1.6,-.65,.8,.65,.95)]):
        id=65070+cluster*3+piece;n=9;vs=[]
        cx=x+dx*math.cos(turn)-dz*math.sin(turn)
        cz=z+dx*math.sin(turn)+dz*math.cos(turn)
        for row,(height,width) in enumerate([(10.25,.75),(11.35,1),(10.25+h,.53)]):
            for j in range(n):
                a=j*math.tau/n;w=1+.16*math.sin(j*7.1+id)
                px,pz=rx*w*width*math.sin(a),rz*w*width*math.cos(a)
                vs.append((cx+px*math.cos(turn)-pz*math.sin(turn),
                    height+(0 if row==0 else .12*math.sin(j*4.3+id)),
                    cz+px*math.sin(turn)+pz*math.cos(turn)))
        fs=[tuple(range(n-1,-1,-1)),tuple(range(2*n,3*n))]
        fs += [(r*n+j,r*n+(j+1)%n,(r+1)*n+(j+1)%n,(r+1)*n+j) for r in range(2) for j in range(n)]
        solid_record(id,vs,fs)
        mesh('Stillwater fractured bank '+str(id),vs,fs,shore_preview)
# Authored basin buttresses: a large leaning slab and two lower talus pieces at each
# site, clear of the village gates, switchbacks and water. The shared rock presenter
# draws these collision facets directly, independent of the settlement glTF and visual LOD.
basin_rock=bpy.data.materials.get('Greyhaven game rock - terrain context') or shore_preview
basin_rock['tile_metres']=2.7
for cluster,(x,z,rx,rz,h,turn,lean_x,lean_z) in enumerate([(-37,-40,4.2,5.0,15.5,.2,-5,-3),
    (-39,57,3.0,4.4,10.5,-.3,-5,0),(-25,83,3.6,4.8,11.5,.6,-1,5)]):
    for piece,(dx,dz,size,rise) in enumerate([(0,0,1,1),(3.1,2.1,.48,.43),(-3.0,2.4,.40,.28)]):
        id=65094+cluster*3+piece;n=9;vs=[]
        cx=x+dx*math.cos(turn)-dz*math.sin(turn)
        cz=z+dx*math.sin(turn)+dz*math.cos(turn)
        for row,(height,width,lean) in enumerate([(-.8,.72,0),(.18*h*rise,1,.20),
            (.63*h*rise,.78,-.35),(h*rise,.42,-.8)]):
            for j in range(n):
                a=j*math.tau/n;w=1+.09*math.sin(j*7.1+id)
                px,pz=rx*size*w*width*math.sin(a),rz*size*w*width*math.cos(a)+lean
                vs.append((cx+px*math.cos(turn)-pz*math.sin(turn)+lean_x*size*row/3,
                    height+(0 if row==0 else .20*rise*math.sin(j*2.1+id)),
                    cz+px*math.sin(turn)+pz*math.cos(turn)+lean_z*size*row/3))
        fs=[tuple(range(n-1,-1,-1)),tuple(range(3*n,4*n))]
        fs += [(r*n+j,r*n+(j+1)%n,(r+1)*n+(j+1)%n,(r+1)*n+j) for r in range(3) for j in range(n)]
        solid_record(id,vs,fs)
        mesh('Greyhaven basin buttress '+str(id),vs,fs,basin_rock)
COLLISION.write_text(json.dumps(collision,separators=(',',':'))+'\n')
print('Authored',len(o.data.polygons),'faces;',len(collision),'simplified collision meshes;',len(resources),'resources.')

# Authoring-only preview of the existing shared animated sail assembly; excluded from glTF.
# The game keeps Motion::Sail and reduced-motion behaviour in mmorpg-presentation.
preview_name='Millbrook sails - game animation preview'
old=bpy.data.collections.get(preview_name)
if old:
    for obj in list(old.objects):bpy.data.objects.remove(obj,do_unlink=True)
    bpy.data.collections.remove(old)
collection=bpy.data.collections.new(preview_name);bpy.context.scene.collection.children.link(collection)
canvas=flat('sail linen preview',(.65,.58,.44))
start,solid_start=len(objects),len(collision)
box('Mill animated hub preview',(0,8.9,2.85),(.32,.32,.22),wood_mat)
for arm in range(4):
    a=arm*math.pi/2;begin=len(objects)
    box('Mill sail spar preview',(0,12.0,2.90),(.1,3.1,.1),wood_mat)
    box('Mill sail linen preview',(.62,12.5,2.93),(.5,2.3,.03),canvas)
    for y in [1.6,2.8,4.,5.2]:box('Mill sail lath preview',(.62,8.9+y,2.97),(.56,.04,.03),wood_mat)
    for obj in objects[begin:]:
        for v in obj.data.vertices:
            x,y=v.co.x,v.co.z-8.9;v.co.x=x*math.cos(a)-y*math.sin(a);v.co.z=8.9+x*math.sin(a)+y*math.cos(a)
farm_place(start,solid_start,88,21,2)
