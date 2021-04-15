# Addr2line Breakpad

addr2line for breakpad symbol file

# Usage

```
> ./breakpad/dump_syms libunity.so > libunity.so.sym
> addr2line_breakpad libunity.so.sym 0000000000c1d21c
0xc1d21c bool UnityDefaultAllocator<LowLevelAllocator>::AllocationPage<(RequestType)0>(void const*) const ??:?
```

# Build

cargo rustc -- -C link-args="resources.res"