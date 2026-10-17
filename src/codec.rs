use crate::abi::*;
use miniz_oxide::inflate::{core::{decompress, DecompressorOxide, inflate_flags::*}, TINFLStatus};
use miniz_oxide::deflate::core::{compress_to_output, CompressorOxide, TDEFLFlush, TDEFLStatus};

#[derive(Clone, Copy, Debug)]
pub struct Error(pub u32, pub u32, pub u32, pub u32);
pub type Result<T, E = Error> = std::result::Result<T, E>;
pub fn error(status: u32, field: &[u8; 4], offset: usize) -> Error { Error(status, 1, u32::from_be_bytes(*field), offset as u32) }
pub fn bytes(n: usize, limit: usize) -> Result<Vec<u8>> {
    if n > limit { return Err(error(STATUS_RESOURCE_LIMIT, b"size", n)); }
    let mut v = Vec::new(); v.try_reserve_exact(n).map_err(|_| error(STATUS_ALLOCATION_FAILURE, b"size", n))?; v.resize(n, 0); Ok(v)
}
fn append(v: &mut Vec<u8>, data: &[u8], limit: usize) -> Result<()> {
    let n = v.len().checked_add(data.len()).ok_or(error(STATUS_RESOURCE_LIMIT, b"size", 0))?;
    if n > limit { return Err(error(STATUS_RESOURCE_LIMIT, b"size", n)); }
    v.try_reserve(data.len()).map_err(|_| error(STATUS_ALLOCATION_FAILURE, b"size", n))?; v.extend_from_slice(data); Ok(())
}
fn be32(b: &[u8]) -> u32 { u32::from_be_bytes(b[..4].try_into().unwrap()) }
fn be16(b: &[u8]) -> u32 { u16::from_be_bytes(b[..2].try_into().unwrap()) as u32 }
fn size(n: u64, limit: usize) -> Result<usize> { if n > limit as u64 { Err(error(STATUS_RESOURCE_LIMIT, b"size", 0)) } else { Ok(n as usize) } }
fn product(values: &[usize]) -> Result<u64> {
    values.iter().try_fold(1u64, |total, &value|
        total.checked_mul(value as u64).ok_or(error(STATUS_RESOURCE_LIMIT,b"size",0)))
}
fn scanline_bytes(width: usize, channels: usize, depth: usize) -> Result<usize> {
    let bits=product(&[width,channels,depth])?;
    let rounded=bits.checked_add(7).ok_or(error(STATUS_RESOURCE_LIMIT,b"size",0))?;
    size(rounded/8,LIMIT_DECODED_BYTES)
}
const fn crc_table() -> [u32;256] {
    let mut table=[0;256];let mut n=0;
    while n<256 {let mut c=n as u32;let mut bit=0;while bit<8 {c=(c>>1)^(0xedb88320 & 0u32.wrapping_sub(c&1));bit+=1;}table[n]=c;n+=1;}
    table
}
const CRC_TABLE: [u32;256] = crc_table();
fn crc(b: &[u8]) -> u32 {
    let mut c = !0u32; for &b in b { c = CRC_TABLE[((c ^ b as u32) & 255) as usize] ^ (c >> 8); } !c
}
fn inflate(input: &[u8], ceiling: usize, exact: bool, field: &[u8;4]) -> Result<Vec<u8>> {
    let mut out = bytes(if exact { ceiling } else { ceiling.min(4096) }, LIMIT_DECODED_BYTES)?;
    let mut decoder = DecompressorOxide::new();
    let (mut consumed, mut written) = (0, 0);
    loop {
        let (status, used, produced) = decompress(&mut decoder, &input[consumed..], &mut out, written,
            TINFL_FLAG_PARSE_ZLIB_HEADER | TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF | TINFL_FLAG_COMPUTE_ADLER32);
        consumed += used; written += produced;
        if status == TINFLStatus::HasMoreOutput {
            if exact || out.len() == ceiling { return Err(error(if exact { STATUS_MALFORMED } else { STATUS_RESOURCE_LIMIT }, field, consumed)); }
            let next = out.len().checked_mul(2).ok_or(error(STATUS_RESOURCE_LIMIT,field,consumed))?.min(ceiling);
            out.try_reserve_exact(next - out.len()).map_err(|_| error(STATUS_ALLOCATION_FAILURE, field, consumed))?;
            out.resize(next, 0);
            continue;
        }
        if status != TINFLStatus::Done || consumed != input.len() || (exact && written != ceiling) { return Err(error(STATUS_MALFORMED, field, consumed)); }
        out.truncate(written); return Ok(out);
    }
}
// One retained compressor is initialized before any result can be published.
// Resetting it performs no allocation; bounded callback output uses try_reserve.
fn deflate(input: &[u8], compressor: &mut CompressorOxide) -> Result<Vec<u8>> {
    compressor.reset();
    let mut out = Vec::new();
    let mut failure = None;
    let (status, consumed) = compress_to_output(compressor, input, TDEFLFlush::Finish, |part| {
        match append(&mut out, part, LIMIT_OUTPUT_BYTES) {
            Ok(()) => true,
            Err(e) => { failure = Some(e); false }
        }
    });
    if let Some(e) = failure { return Err(e); }
    if status != TDEFLStatus::Done || consumed != input.len() {
        return Err(error(STATUS_INTERNAL_FAILURE, b"zlib", consumed));
    }
    Ok(out)
}
fn chunk(out: &mut Vec<u8>, kind: &[u8;4], data: &[u8]) -> Result<()> {
    let length=u32::try_from(data.len()).map_err(|_|error(STATUS_RESOURCE_LIMIT,kind,0))?;
    append(out, &length.to_be_bytes(), LIMIT_OUTPUT_BYTES)?;
    let start = out.len(); append(out, kind, LIMIT_OUTPUT_BYTES)?; append(out, data, LIMIT_OUTPUT_BYTES)?;
    let check = crc(&out[start..]); append(out, &check.to_be_bytes(), LIMIT_OUTPUT_BYTES)
}
pub struct Image { pub record: [u32; RECORD_WORDS], pub pixels: Vec<u8>, pub profile: Vec<u8> }
const PASSES: [(usize,usize,usize,usize);7] = [(0,0,8,8),(4,0,8,8),(0,4,4,8),(2,0,4,4),(0,2,2,4),(1,0,2,2),(0,1,1,2)];
fn pass_size(w: usize, h: usize, p: (usize,usize,usize,usize)) -> (usize,usize) { (w.saturating_sub(p.0).div_ceil(p.2), h.saturating_sub(p.1).div_ceil(p.3)) }
fn paeth(a: u8,b: u8,c: u8) -> u8 { let p=a as i32+b as i32-c as i32; let (pa,pb,pc)=((p-a as i32).abs(),(p-b as i32).abs(),(p-c as i32).abs()); if pa<=pb && pa<=pc {a} else if pb<=pc {b} else {c} }
pub fn decode(input: &[u8]) -> Result<Image> {
    if input.len()>LIMIT_INPUT_BYTES { return Err(error(STATUS_RESOURCE_LIMIT,b"size",0)); }
    if !input.starts_with(&[137,80,78,71,13,10,26,10]) { return Err(error(STATUS_MALFORMED,b"sign",0)); }
    let mut r=[0u32;RECORD_WORDS]; r[F_VERSION]=VERSION; r[F_WHITE_POINT]=WHITE_POINT_D65;
    let (mut w,mut h,mut depth,mut ct,mut interlace,mut channels)=(0usize,0usize,0usize,0u8,0u8,0usize);
    let mut decoded_bytes=0;
    let mut colors: [Option<&[u8]>;7]=[None;7]; let kinds=[*b"cICP",*b"iCCP",*b"sRGB",*b"gAMA",*b"cHRM",*b"mDCv",*b"cLLi"];
    let (mut palette,mut alpha)=(None::<&[u8]>,None::<&[u8]>); let mut idat=Vec::new();
    let (mut at,mut data_state,mut ended)=(8usize,0u8,false);
    while at<input.len() {
        if input.len()-at<12 { return Err(error(STATUS_MALFORMED,b"head",at)); }
        let len=be32(&input[at..]) as usize; let end=at.checked_add(len).and_then(|n|n.checked_add(12)).filter(|&n|n<=input.len()).ok_or(error(STATUS_MALFORMED,b"size",at))?;
        let kind: &[u8;4]=input[at+4..at+8].try_into().unwrap();
        if !kind.iter().all(u8::is_ascii_alphabetic) || !kind[2].is_ascii_uppercase() { return Err(error(STATUS_MALFORMED,kind,at)); }
        if crc(&input[at+4..end-4])!=be32(&input[end-4..end]) { return Err(error(STATUS_MALFORMED,kind,at)); }
        let d=&input[at+8..end-4];
        if at==8 && kind!=b"IHDR" { return Err(error(STATUS_MALFORMED,kind,at)); }
        if kind!=b"IDAT" && data_state==1 { data_state=2; }
        match kind {
            b"IHDR" => {
                if at!=8 || len!=13 { return Err(error(STATUS_MALFORMED,kind,at)); }
                w=be32(d) as usize; h=be32(&d[4..]) as usize; depth=d[8] as usize; ct=d[9]; interlace=d[12];
                if w==0 || h==0 || w>0x7fffffff || h>0x7fffffff { return Err(error(STATUS_MALFORMED,kind,at)); }
                channels=match ct {0=>1,2=>3,3=>1,4=>2,6=>4,_=>return Err(error(STATUS_UNSUPPORTED,kind,at))};
                let valid=match ct {0=>[1,2,4,8,16].contains(&depth),3=>[1,2,4,8].contains(&depth),_=>[8,16].contains(&depth)};
                if !valid { return Err(error(STATUS_MALFORMED,kind,at)); }
                if d[10]!=0 || d[11]!=0 || interlace>1 { return Err(error(STATUS_UNSUPPORTED,kind,at)); }
                decoded_bytes=size(product(&[w,h,if ct==3 {4} else {channels},depth.div_ceil(8)])?,LIMIT_DECODED_BYTES)?;
            },
            b"PLTE" => { if palette.is_some() || data_state!=0 || alpha.is_some() || ct==0 || ct==4 || len==0 || len%3!=0 || len>768 || (ct==3 && len/3>1usize<<depth) {return Err(error(STATUS_MALFORMED,kind,at));} palette=Some(d); },
            b"tRNS" => { if ct!=3 {return Err(error(STATUS_UNSUPPORTED,kind,at));} if alpha.is_some() || data_state!=0 || palette.is_none() || len==0 || len>palette.unwrap().len()/3 {return Err(error(STATUS_MALFORMED,kind,at));} alpha=Some(d); },
            b"IDAT" => { if data_state==2 || (ct==3 && palette.is_none()) {return Err(error(STATUS_MALFORMED,kind,at));} data_state=1; append(&mut idat,d,LIMIT_INPUT_BYTES)?; },
            b"IEND" => { if len!=0 || data_state==0 || end!=input.len() {return Err(error(STATUS_MALFORMED,kind,at));} ended=true; at=end; break; },
            _ => {
                if let Some(i)=kinds.iter().position(|k|k==kind) {
                    if colors[i].is_some() || data_state!=0 || (i<=4 && palette.is_some()) {return Err(error(STATUS_MALFORMED,kind,at));}
                    let expected=[4,0,1,4,32,24,8][i]; if i!=1 && len!=expected {return Err(error(STATUS_MALFORMED,kind,at));}
                    if i==2 && d[0]>3 {return Err(error(STATUS_MALFORMED,kind,at));}
                    if i==3 && be32(d)==0 {return Err(error(STATUS_MALFORMED,kind,at));}
                    colors[i]=Some(d);
                } else if kind[0].is_ascii_uppercase() {return Err(error(STATUS_UNSUPPORTED,kind,at));}
            }
        }
        at=end;
    }
    if !ended || at!=input.len() { return Err(error(STATUS_MALFORMED,b"IEND",at)); }
    if colors[1].is_some() && colors[2].is_some() { return Err(error(STATUS_MALFORMED,b"iCCP",0)); }
    let mut profile=Vec::new();
    if let Some(d)=colors[1] {
        let n=d.iter().position(|&b|b==0).ok_or(error(STATUS_MALFORMED,b"iCCP",0))?;
        if n==0 || n>79 || n+2>d.len() || d[n+1]!=0 || d[0]==32 || d[n-1]==32 || d[..n].windows(2).any(|p|p==b"  ") || d[..n].iter().any(|&b| !(32..=126).contains(&b) && b<161) { return Err(error(STATUS_MALFORMED,b"iCCP",0)); }
        profile=inflate(&d[n+2..],LIMIT_PROFILE_BYTES,false,b"iCCP")?;
    }
    let single=[(0,0,1,1)]; let passes=if interlace==1 {&PASSES[..]} else {&single[..]};
    let mut expected=0u64;
    for &p in passes {
        let (pw,ph)=pass_size(w,h,p); if pw==0 || ph==0 {continue;}
        let stride=(scanline_bytes(pw,channels,depth)? as u64).checked_add(1)
            .ok_or(error(STATUS_RESOURCE_LIMIT,b"size",0))?;
        let length=stride.checked_mul(ph as u64).ok_or(error(STATUS_RESOURCE_LIMIT,b"size",0))?;
        expected=expected.checked_add(length).ok_or(error(STATUS_RESOURCE_LIMIT,b"size",0))?;
    }
    let raw=inflate(&idat,size(expected,LIMIT_DECODED_BYTES)?,true,b"IDAT")?;
    let out_channels=if ct==3 {4} else {channels}; let bps=depth.div_ceil(8);
    let mut pixels=bytes(decoded_bytes,LIMIT_DECODED_BYTES)?;
    let bpp=(channels*depth).div_ceil(8).max(1); let mut cursor=0;
    for &p in passes {
        let (pw,ph)=pass_size(w,h,p); if pw==0 || ph==0 {continue;}
        let row_bytes=scanline_bytes(pw,channels,depth)?; let mut prev=bytes(row_bytes,LIMIT_DECODED_BYTES)?; let mut row=bytes(row_bytes,LIMIT_DECODED_BYTES)?;
        for y in 0..ph {
            let filter=raw[cursor]; if filter>4 {return Err(error(STATUS_MALFORMED,b"filt",cursor));} cursor+=1;
            for x in 0..row_bytes {let a=if x>=bpp {row[x-bpp]} else {0}; let b=prev[x]; let c=if x>=bpp {prev[x-bpp]} else {0};
                row[x]=raw[cursor+x].wrapping_add(match filter {0=>0,1=>a,2=>b,3=>((a as u16+b as u16)/2) as u8,_=>paeth(a,b,c)});
            } cursor+=row_bytes;
            for x in 0..pw { let dest=((p.1+y*p.3)*w+p.0+x*p.2)*out_channels*bps;
                if ct==3 {let index=if depth==8 {row[x] as usize} else {((row[x*depth/8]>>(8-depth-(x*depth%8)))&((1<<depth)-1)) as usize};
                    let pal=palette.unwrap(); if index>=pal.len()/3 {return Err(Error(STATUS_MALFORMED,2,u32::from_be_bytes(*b"PLTE"),(dest/4) as u32));}
                    pixels[dest..dest+3].copy_from_slice(&pal[index*3..index*3+3]); pixels[dest+3]=alpha.and_then(|a|a.get(index)).copied().unwrap_or(255);
                } else if depth<8 {let value=(row[x*depth/8]>>(8-depth-x*depth%8))&((1<<depth)-1); pixels[dest]=(value as u32*255/((1<<depth)-1)) as u8;
                } else {for c in 0..channels {let src=(x*channels+c)*bps; if depth==16 {pixels[dest+c*2]=row[src+1];pixels[dest+c*2+1]=row[src];} else {pixels[dest+c]=row[src];}}}
            }
            std::mem::swap(&mut row,&mut prev);
        }
    }
    r[F_WIDTH]=w as u32;r[F_HEIGHT]=h as u32;r[F_ROW_STRIDE]=(w*out_channels*bps) as u32;r[F_SAMPLE_TYPE]=if depth==16 {SAMPLE_TYPE_UINT16} else {SAMPLE_TYPE_UINT8};r[F_BITS_PER_SAMPLE]=if ct==3 {8} else {depth as u32};
    r[F_CHANNEL_LAYOUT]=match ct {0=>CHANNEL_LAYOUT_GRAY,2=>CHANNEL_LAYOUT_RGB,4=>CHANNEL_LAYOUT_GRAY_ALPHA,_=>CHANNEL_LAYOUT_RGBA};r[F_ALPHA_MODE]=if ct==0 || ct==2 {ALPHA_MODE_OPAQUE} else {ALPHA_MODE_STRAIGHT};
    if let Some(d)=colors[5] {r[F_FLAGS]|=FLAG_MASTERING_DISPLAY;for i in 0..8 {r[F_RED_X+i]=be16(&d[i*2..]);}r[F_MAXIMUM_LUMINANCE]=be32(&d[16..]);r[F_MINIMUM_LUMINANCE]=be32(&d[20..]);}
    if let Some(d)=colors[6] {r[F_FLAGS]|=FLAG_CONTENT_LIGHT_LEVEL;r[F_MAXIMUM_CONTENT_LIGHT_LEVEL]=be32(d);r[F_MAXIMUM_FRAME_AVERAGE_LIGHT_LEVEL]=be32(&d[4..]);}
    if let Some(d)=colors[0] {
        r[F_PRIMARIES]=match d[0] {1=>PRIMARIES_SRGB,9=>PRIMARIES_REC2020,12=>PRIMARIES_DISPLAY_P3,_=>return Err(error(STATUS_UNSUPPORTED,b"cICP",0))};
        r[F_TRANSFER]=match d[1] {1|6|14|15=>TRANSFER_REC2020,8=>TRANSFER_LINEAR,13=>TRANSFER_SRGB,16=>TRANSFER_PQ,18=>TRANSFER_HLG,_=>return Err(error(STATUS_UNSUPPORTED,b"cICP",1))};
        if d[2]!=0 || d[3]!=1 {return Err(error(STATUS_UNSUPPORTED,b"cICP",2));}
        r[F_FLAGS]|=FLAG_CODED_COLOR_VOLUME;r[F_PRIMARIES_CODE]=d[0] as u32;r[F_TRANSFER_CODE]=d[1] as u32;profile.clear();
    } else if colors[1].is_some() {r[F_FLAGS]|=FLAG_ICC_PROFILE;r[F_TRANSFER]=TRANSFER_ICC;
    } else if colors[2].is_none() {
        if let Some(d)=colors[4] {let known=[[31270,32900,64000,33000,30000,60000,15000,6000],[31270,32900,68000,32000,26500,69000,15000,6000],[31270,32900,70800,29200,17000,79700,13100,4600],[31270,32900,64000,33000,21000,71000,15000,6000],[34570,35850,73470,26530,15960,84040,3660,10]];
            r[F_PRIMARIES]=known.iter().position(|k|k.iter().enumerate().all(|(i,&v)|be32(&d[i*4..]).abs_diff(v)<=300)).ok_or(error(STATUS_UNSUPPORTED,b"cHRM",0))? as u32;
            if r[F_PRIMARIES]==PRIMARIES_PROPHOTO_RGB {r[F_WHITE_POINT]=WHITE_POINT_D50;}
        }
        if let Some(d)=colors[3] {r[F_TRANSFER]=TRANSFER_GAMMA;let g=(100000f64/be32(d) as f64).to_bits();r[F_GAMMA_LOW]=g as u32;r[F_GAMMA_HIGH]=(g>>32) as u32;
        } else {r[F_FLAGS]|=if colors[4].is_some() {FLAG_ASSUMED_SRGB_TRANSFER} else {FLAG_ASSUMED_SRGB};}
    }
    Ok(Image{record:r,pixels,profile})
}

fn luminance(r: &[u32;RECORD_WORDS], field: usize) -> Result<u32> {
    let value=f64::from_bits(r[field] as u64 | ((r[field+1] as u64)<<32));
    let scaled=(value*10000.0+0.5).floor();
    if !value.is_finite() || value<0.0 || scaled>u32::MAX as f64 {return Err(Error(STATUS_INVALID_REQUEST,3,field as u32,0));}
    Ok(scaled as u32)
}
pub fn encode(r: &[u32;RECORD_WORDS], pixels: &[u8], profile: &[u8], compressor: &mut CompressorOxide) -> Result<Vec<u8>> {
    let bad=error(STATUS_INVALID_REQUEST,b"desc",0);
    if r[F_VERSION]!=VERSION {return Err(error(STATUS_ABI_MISMATCH,b"vers",0));}
    let (w,h,depth)=(r[F_WIDTH] as usize,r[F_HEIGHT] as usize,r[F_BITS_PER_SAMPLE] as usize);
    // RGBA with straight alpha encodes as color type 6; RGB, which has no
    // alpha, as color type 2.
    let (channels,color_type)=match (r[F_CHANNEL_LAYOUT],r[F_ALPHA_MODE]) {(CHANNEL_LAYOUT_RGBA,ALPHA_MODE_STRAIGHT)=>(4usize,6u8),(CHANNEL_LAYOUT_RGB,ALPHA_MODE_OPAQUE)=>(3,2),_=>return Err(bad)};
    if w==0 || h==0 || w>0x7fffffff || h>0x7fffffff || ![8,16].contains(&depth) || r[F_MODE]>1 || r[F_COMPRESSION_LEVEL]>9 {return Err(bad);}
    let source_bps=match r[F_SAMPLE_TYPE] {SAMPLE_TYPE_UINT8=>1,SAMPLE_TYPE_UINT16=>2,SAMPLE_TYPE_FLOAT32=>4,_=>return Err(bad)};
    if (r[F_SAMPLE_TYPE]==SAMPLE_TYPE_UINT8 && depth!=8) || (r[F_SAMPLE_TYPE]==SAMPLE_TYPE_UINT16 && depth!=16) {return Err(bad);}
    if r[F_PRIMARIES] as usize >= PRIMARIES_NAMES.len()
        || r[F_TRANSFER] as usize >= TRANSFER_NAMES.len()
        || r[F_WHITE_POINT] as usize >= WHITE_POINT_NAMES.len()
        || r[F_FLAGS] & !(FLAG_MASTERING_DISPLAY | FLAG_CONTENT_LIGHT_LEVEL) != 0 {
        return Err(bad);
    }
    let stride=r[F_ROW_STRIDE] as usize;let row_in=size(product(&[w,channels,source_bps])?,LIMIT_DECODED_BYTES)?;
    let extent=product(&[h-1,stride])?.checked_add(row_in as u64)
        .ok_or(error(STATUS_RESOURCE_LIMIT,b"size",0))?;
    if stride<row_in || stride%source_bps!=0 || pixels.len() as u64!=extent {return Err(bad);}
    if r[F_PIXEL_OFFSET] as usize%source_bps!=0 {return Err(bad);}
    let row_out=size(product(&[w,channels,depth/8])?,LIMIT_DECODED_BYTES)?;
    let filtered_stride=row_out.checked_add(1).ok_or(error(STATUS_RESOURCE_LIMIT,b"size",0))?;
    let n=size(product(&[filtered_stride,h])?,LIMIT_DECODED_BYTES)?;
    let mut rows=bytes(n,LIMIT_DECODED_BYTES)?;
    for y in 0..h {let base=y*(row_out+1);rows[base]=1;
        for s in 0..w*channels {let from=y*stride+s*source_bps;let q=match r[F_SAMPLE_TYPE] {SAMPLE_TYPE_UINT8=>pixels[from] as u16,SAMPLE_TYPE_UINT16=>u16::from_le_bytes(pixels[from..from+2].try_into().unwrap()),_=>{
                let f=f32::from_le_bytes(pixels[from..from+4].try_into().unwrap()) as f64;let f=if f.is_nan(){0.0}else{f.clamp(0.0,1.0)};(f*(if depth==16 {65535.0}else{255.0})+0.5).floor() as u16
            }};
            // Store the unfiltered row first; filter backwards so the left sample stays intact.
            if depth==16 {rows[base+1+s*2..base+3+s*2].copy_from_slice(&q.to_be_bytes());}else{rows[base+1+s]=q as u8;}
        }
        let bpp=channels*(depth/8);for x in (bpp..row_out).rev(){rows[base+1+x]=rows[base+1+x].wrapping_sub(rows[base+1+x-bpp]);}
    }
    let mut out=Vec::new();append(&mut out,&[137,80,78,71,13,10,26,10],LIMIT_OUTPUT_BYTES)?;
    let mut ihdr=[0u8;13];ihdr[..4].copy_from_slice(&(w as u32).to_be_bytes());ihdr[4..8].copy_from_slice(&(h as u32).to_be_bytes());ihdr[8]=depth as u8;ihdr[9]=color_type;chunk(&mut out,b"IHDR",&ihdr)?;
    if r[F_MODE]==1 {
        match (r[F_PRIMARIES],r[F_TRANSFER]) {
            (PRIMARIES_SRGB,TRANSFER_SRGB)=>chunk(&mut out,b"sRGB",&[0])?,
            (PRIMARIES_DISPLAY_P3,TRANSFER_SRGB)=>{if profile.is_empty() || profile.len()>LIMIT_PROFILE_BYTES{return Err(bad);}let compressed=deflate(profile, compressor)?;let mut d=Vec::new();append(&mut d,b"Display P3\0\0",LIMIT_OUTPUT_BYTES)?;append(&mut d,&compressed,LIMIT_OUTPUT_BYTES)?;chunk(&mut out,b"iCCP",&d)?;},
            (PRIMARIES_REC2020,TRANSFER_PQ|TRANSFER_HLG)=>chunk(&mut out,b"cICP",&[9,if r[F_TRANSFER]==TRANSFER_PQ {16}else{18},0,1])?,
            _=>return Err(bad),
        }
        if r[F_FLAGS]&FLAG_MASTERING_DISPLAY!=0 {
            let mut d=[0u8;24];
            let chromaticities=[35400u16,14600,8500,39850,6550,2300,15635,16450];
            for (i,value) in chromaticities.iter().enumerate(){d[i*2..i*2+2].copy_from_slice(&value.to_be_bytes());}
            d[16..20].copy_from_slice(&luminance(r,F_MASTERING_MAX_LOW)?.to_be_bytes());
            d[20..24].copy_from_slice(&luminance(r,F_MASTERING_MIN_LOW)?.to_be_bytes());
            chunk(&mut out,b"mDCv",&d)?;
        }
        if r[F_FLAGS]&FLAG_CONTENT_LIGHT_LEVEL!=0 {
            let mut d=[0u8;8];d[..4].copy_from_slice(&luminance(r,F_CONTENT_MAX_LOW)?.to_be_bytes());
            d[4..].copy_from_slice(&luminance(r,F_FRAME_AVERAGE_LOW)?.to_be_bytes());chunk(&mut out,b"cLLi",&d)?;
        }
    // An untagged image (a float16 sRGB canvas's 16-bit PNG, as Chrome
    // writes it) carries no color chunk, profile or metadata.
    } else if r[F_FLAGS]!=0 || !profile.is_empty() {return Err(bad);}
    chunk(&mut out,b"IDAT",&deflate(&rows, compressor)?)?;chunk(&mut out,b"IEND",&[])?;Ok(out)
}
