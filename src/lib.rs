#![allow(dead_code)]
mod abi;
mod codec;

#[cfg(target_arch = "wasm32")]
mod exports {
    use super::{abi::*, codec::{self, Error, Result}};
    use miniz_oxide::deflate::core::{CompressorOxide, create_comp_flags_from_zip_params};
    use std::cell::RefCell;

    struct Allocation { id: u32, role: u8, words: Vec<u32>, length: usize }
    impl Allocation {
        fn bytes(&self) -> &[u8] { unsafe { std::slice::from_raw_parts(self.words.as_ptr().cast(), self.length) } }
        fn bytes_mut(&mut self) -> &mut [u8] { unsafe { std::slice::from_raw_parts_mut(self.words.as_mut_ptr().cast(), self.length) } }
    }
    struct State { allocations: Vec<Allocation>, next: u32, retained: usize, compressor: CompressorOxide }
    thread_local! {
        static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
        static ERROR: RefCell<[u32;4]> = const { RefCell::new([0;4]) };
    }
    fn fail(e: Error) -> u32 { ERROR.with(|r| *r.borrow_mut()=[e.0,e.1,e.2,e.3]); 0 }
    fn bad_handle() -> Error { Error(STATUS_INVALID_HANDLE,0,0,0) }
    impl State {
        fn get(&self,id:u32,role:u8)->Result<&Allocation>{self.allocations.iter().find(|a|a.id==id && a.role==role).ok_or(bad_handle())}
        fn allocate(&mut self,len:usize,role:u8)->Result<u32>{
            let total=self.retained.checked_add(len).filter(|&n|n<=LIMIT_RETAINED_BYTES)
                .ok_or(Error(STATUS_RESOURCE_LIMIT,0,0,0))?;
            if len>LIMIT_OUTPUT_BYTES+RECORD_WORDS*4+LIMIT_PROFILE_BYTES {return Err(Error(STATUS_RESOURCE_LIMIT,0,0,0));}
            let id=self.next.checked_add(1).ok_or(Error(STATUS_RESOURCE_LIMIT,0,0,0))?;
            let word_count=len.checked_add(3).ok_or(Error(STATUS_RESOURCE_LIMIT,0,0,0))?/4;
            let mut words=Vec::new();words.try_reserve_exact(word_count).map_err(|_|Error(STATUS_ALLOCATION_FAILURE,0,0,0))?;words.resize(word_count,0);
            self.allocations.try_reserve(1).map_err(|_|Error(STATUS_ALLOCATION_FAILURE,0,0,0))?;
            self.allocations.push(Allocation{id,role,words,length:len});self.next=id;self.retained=total;Ok(id)
        }
        fn release(&mut self,id:u32)->Result<u32>{let i=self.allocations.iter().position(|a|a.id==id).ok_or(bad_handle())?;let a=self.allocations.swap_remove(i);self.retained-=a.length;Ok(1)}
        fn publish(&mut self,mut image:codec::Image)->Result<u32>{
            let header=RECORD_WORDS*4;
            let pixel_end=header.checked_add(image.pixels.len()).ok_or(Error(STATUS_RESOURCE_LIMIT,0,0,0))?;
            let profile_at=pixel_end.checked_add(3).ok_or(Error(STATUS_RESOURCE_LIMIT,0,0,0))? & !3;
            let total=profile_at.checked_add(image.profile.len()).ok_or(Error(STATUS_RESOURCE_LIMIT,0,0,0))?;
            let id=self.allocate(total,2)?;
            let ptr=self.allocations.last().unwrap().words.as_ptr() as u32;
            let pointers=(ptr.checked_add(header as u32),ptr.checked_add(profile_at as u32));
            let (Some(pixel_pointer),Some(profile_pointer))=pointers else {
                self.release(id)?;return Err(Error(STATUS_INTERNAL_FAILURE,0,0,0));
            };
            let a=self.allocations.last_mut().unwrap();
            image.record[F_PIXEL_OFFSET]=pixel_pointer;image.record[F_PIXEL_LENGTH]=image.pixels.len() as u32;
            image.record[F_PROFILE_OFFSET]=if image.profile.is_empty(){0}else{profile_pointer};image.record[F_PROFILE_LENGTH]=image.profile.len() as u32;
            a.words[..RECORD_WORDS].copy_from_slice(&image.record);
            a.bytes_mut()[header..pixel_end].copy_from_slice(&image.pixels);
            a.bytes_mut()[profile_at..].copy_from_slice(&image.profile);Ok(id)
        }
        fn span(&self,id:u32,offset:u32,len:u32)->Result<&[u8]>{let a=self.get(id,1)?;let end=(offset as u64).checked_add(len as u64).ok_or(Error(STATUS_RESOURCE_LIMIT,0,0,offset))?;if end>a.length as u64{return Err(Error(STATUS_INVALID_REQUEST,0,0,offset));}Ok(&a.bytes()[offset as usize..end as usize])}
        fn decode(&mut self,id:u32)->Result<u32>{let image=codec::decode(self.get(id,1)?.bytes())?;self.publish(image)}
        fn encode(&mut self,id:u32)->Result<u32>{
            let a=self.get(id,1)?;if a.length!=RECORD_WORDS*4{return Err(Error(STATUS_ABI_MISMATCH,0,0,0));}
            let r:[u32;RECORD_WORDS]=a.words[..RECORD_WORDS].try_into().unwrap();
            if r[F_VERSION]!=VERSION{return Err(Error(STATUS_ABI_MISMATCH,0,0,0));}
            // Allocation buffers never move when the registry changes. These scoped slices
            // end before publication; the compressor cannot access or grow that registry.
            let pixels=self.span(r[F_PIXEL_HANDLE],r[F_PIXEL_OFFSET],r[F_PIXEL_LENGTH])?;
            let profile=if r[F_PROFILE_LENGTH]==0 {&[][..]}else{self.span(r[F_PROFILE_HANDLE],r[F_PROFILE_OFFSET],r[F_PROFILE_LENGTH])?};
            let (pp,pl,cp,cl)=(pixels.as_ptr(),pixels.len(),profile.as_ptr(),profile.len());
            let out=unsafe{codec::encode(&r,std::slice::from_raw_parts(pp,pl),std::slice::from_raw_parts(cp,cl),&mut self.compressor)}?;
            let mut record=[0;RECORD_WORDS];record[F_VERSION]=VERSION;
            self.publish(codec::Image{record,pixels:out,profile:Vec::new()})
        }
    }
    fn call(f:impl FnOnce(&mut State)->Result<u32>)->u32{
        ERROR.with(|e|*e.borrow_mut()=[0;4]);
        STATE.with(|s|match s.try_borrow_mut(){Ok(mut state)=>match state.as_mut(){Some(state)=>match f(state){Ok(v)=>v,Err(e)=>fail(e)},None=>fail(Error(STATUS_ABI_MISMATCH,0,0,0))},Err(_)=>fail(Error(STATUS_INTERNAL_FAILURE,0,0,0))})
    }
    #[no_mangle] pub extern "C" fn png_version()->u32{VERSION}
    #[no_mangle] pub extern "C" fn png_record_bytes()->u32{(RECORD_WORDS*4) as u32}
    #[no_mangle] pub extern "C" fn png_initialize()->u32{
        STATE.with(|s|{let mut state=s.borrow_mut();if state.is_none(){*state=Some(State{allocations:Vec::new(),next:0,retained:0,compressor:CompressorOxide::new(create_comp_flags_from_zip_params(6,15,0))});}1})
    }
    #[no_mangle] pub extern "C" fn png_alloc(len:u32)->u32{call(|s|s.allocate(len as usize,1))}
    #[no_mangle] pub extern "C" fn png_ptr(id:u32)->u32{call(|s|s.allocations.iter().find(|a|a.id==id).map(|a|a.words.as_ptr() as u32).ok_or(bad_handle()))}
    #[no_mangle] pub extern "C" fn png_len(id:u32)->u32{call(|s|s.allocations.iter().find(|a|a.id==id).map(|a|a.length as u32).ok_or(bad_handle()))}
    #[no_mangle] pub extern "C" fn png_release(id:u32)->u32{call(|s|s.release(id))}
    #[no_mangle] pub extern "C" fn png_decode(id:u32)->u32{call(|s|s.decode(id))}
    #[no_mangle] pub extern "C" fn png_encode(id:u32)->u32{call(|s|s.encode(id))}
    #[no_mangle] pub extern "C" fn png_error()->u32{ERROR.with(|e|e.as_ptr() as u32)}
}
