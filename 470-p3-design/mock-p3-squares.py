# MOCK for #470: Okhsl squares (hue across, lightness down) at a fixed saturation, written as
# 16-bit PNGs tagged Display P3, so a P3 screen shows the P3 colors. Not the app's code.
import math, struct, zlib, sys
L2S=[[4.0767416621,-3.3077115913,0.2309699292],[-1.2684380046,2.6097574011,-0.3413193965],[-0.0041960863,-0.7034186147,1.7076147010]]
S2P=[[0.8224621,0.177538,0.0],[0.0331941,0.9668058,0.0],[0.0170827,0.0723974,0.9105199]]
P2S=[[1.2249401,-0.2249404,0.0],[-0.0420569,1.0420571,0.0],[-0.0196376,-0.0786361,1.0982735]]
L2P=[[sum(S2P[i][k]*L2S[k][j] for k in range(3)) for j in range(3)] for i in range(3)]
def mul(M,v): return [sum(M[i][j]*v[j] for j in range(3)) for i in range(3)]
def lin(M,l,a,b):
    kl=0.3963377774*a+0.2158037573*b; km=-0.1055613458*a-0.0638541728*b; ks=-0.0894841775*a-1.2914855480*b
    return mul(M,[(l+kl)**3,(l+km)**3,(l+ks)**3])
def inside(M,l,c,a,b): return all(-1e-7<=x<=1+1e-7 for x in lin(M,l,c*a,c*b))
def cmax(M,l,a,b):
    lo,hi=0.0,0.6
    for _ in range(30):
        m=(lo+hi)/2
        if inside(M,l,m,a,b): lo=m
        else: hi=m
    return lo
K1,K2=0.206,0.03; K3=(1+K1)/(1+K2)
def toe_inv(x): return (x*x+K1*x)/(K3*(x+K2))
cusps={}
def cusp(M,a,b,key):
    if key in cusps: return cusps[key]
    best=(0,0)
    for i in range(1,200):
        l=i/200; c=cmax(M,l,a,b)
        if c>best[1]: best=(l,c)
    cusps[key]=best; return best
def okhsl(M,tag,h,s,l):
    a,b=math.cos(2*math.pi*h),math.sin(2*math.pi*h); L=toe_inv(l)
    cu=cusp(M,a,b,(tag,round(h,4)))
    c_max=cmax(M,L,a,b)
    s_max,t_max=cu[1]/cu[0],cu[1]/(1-cu[0])
    k=c_max/min(L*s_max,(1-L)*t_max)
    s_mid=0.11516993+1/(7.44778970+4.15901240*b+a*(-2.19557347+1.75198401*b+a*(-2.13704948-10.02301043*b+a*(-4.24894561+5.38770819*b+4.69891013*a))))
    t_mid=0.11239642+1/(1.61320320-0.68124379*b+a*(0.40370612+0.90148123*b+a*(-0.27087943+0.61223990*b+a*(0.00299215-0.45399568*b-0.14661872*a))))
    ca,cb=L*s_mid,(1-L)*t_mid
    c_mid=0.9*k*math.sqrt(math.sqrt(1/(1/ca**4+1/cb**4)))
    ca,cb=L*0.4,(1-L)*0.8; c0=math.sqrt(1/(1/ca**2+1/cb**2))
    if s<0.8:
        t,k1=1.25*s,0.8*c0; c=t*k1/(1-(1-k1/c_mid)*t)
    else:
        t,k1=5*(s-0.8),0.2*c_mid*c_mid*1.25*1.25/c0; c=c_mid+t*k1/(1-(1-k1/(c_max-c_mid))*t)
    return L,c,a,b
def enc(x):
    x=min(max(x,0.0),1.0); return x*12.92 if x<=0.0031308 else 1.055*x**(1/2.4)-0.055
def png(path,w,h,rows):
    icc=open("/System/Library/ColorSync/Profiles/Display P3.icc","rb").read()
    def chunk(t,d): return struct.pack(">I",len(d))+t+d+struct.pack(">I",zlib.crc32(t+d)&0xffffffff)
    raw=b"".join(b"\0"+b"".join(struct.pack(">HHH",*[round(enc(v)*65535) for v in px]) for px in r) for r in rows)
    open(path,"wb").write(b"\x89PNG\r\n\x1a\n"+chunk(b"IHDR",struct.pack(">IIBBBBB",w,h,16,2,0,0,0))
        +chunk(b"iCCP",b"Display P3\0\0"+zlib.compress(icc))+chunk(b"IDAT",zlib.compress(raw,9))+chunk(b"IEND",b""))
W,H=304,168
def square(kind,s):
    rows=[]
    for y in range(H):
        r=[]
        for x in range(W):
            h,l=(x+.5)/W,1-(y+.5)/H
            if kind=="srgb":
                L,c,a,b=okhsl(L2S,"s",h,s,l); p=mul(S2P,lin(L2S,L,c*a,c*b))
            else:
                L,c,a,b=okhsl(L2P,"p",h,s,l); p=lin(L2P,L,c*a,c*b)
                srgb=mul(P2S,p)
                if kind=="p3-on-srgb": p=mul(S2P,[min(max(v,0),1) for v in srgb])
                elif kind=="p3-edge":
                    # the sRGB edge: pixels just past it drawn as a thin line
                    out=any(v<-1e-4 or v>1+1e-4 for v in srgb)
                    r.append((p,out)); continue
            r.append(p)
        rows.append(r)
    if kind=="p3-edge":
        g=rows; rows=[]
        for y in range(H):
            rr=[]
            for x in range(W):
                p,o=g[y][x]
                edge=o and any(not g[yy][xx][1] for yy,xx in ((y-1,x),(y+1,x),(y,x-1),(y,x+1)) if 0<=yy<H and 0<=xx<W)
                rr.append([1,1,1] if edge else p)
            rows.append(rr)
    return rows
s=float(sys.argv[1]) if len(sys.argv)>1 else 1.0
for kind in ["srgb","p3","p3-edge","p3-on-srgb"]:
    png(f"mock-{kind}.png",W,H,square(kind,s)); print(kind)
