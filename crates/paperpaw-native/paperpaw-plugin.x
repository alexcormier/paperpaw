/* How a native PaperPaw plugin is linked (see paperpaw-native): its
   read-only data from 0x3C000000, then its code right after it in the
   instruction bus's view of the same memory (0x06000000 further on), so
   the two make one image that keeps its shape wherever it is loaded.

   A plugin's project keeps a copy of it at its top, beside its
   `.cargo/config.toml`, whose rustflags link with it
   (`-C link-arg=-Tpaperpaw-plugin.x`, with `-Wl,` for the X4 Pro's GCC).
   No build script brings it: one in a plugin's dependencies would be
   built for the computer, whose name Cargo hashes into every crate's
   metadata, and a build on Windows would differ from one on Linux. It is
   part of the plugin interface (native::INTERFACE): a copy that differs
   from this one makes plugins that do not run. */

ENTRY(PAPERPAW_ENTRY)

SECTIONS
{
    . = 0x3C000000;
    .rodata : ALIGN(16)
    {
        KEEP(*(.rodata.paperpaw_entry))
        /* The word the loader writes the firmware's table's address into. */
        . = ALIGN(4);
        PAPERPAW_API = .;
        LONG(0)
        *(.rodata .rodata.* .srodata .srodata.*)
        . = ALIGN(16);
    }

    . = . + 0x06000000;
    .text : ALIGN(16)
    {
        *(.literal .text .literal.* .text.*)
    }

    /* Writable data has no place in a plugin (its state lives on the
       kernel's heap): kept here only to be found and refused when the
       file is packed. */
    .data : { *(.data .data.* .sdata .sdata.*) }
    .bss : { *(.bss .bss.* .sbss .sbss.* COMMON) }

    /* The compilers' names stay (`.comment`, not loaded): the file
       keeps which built it, and runs only on firmware built by it. */
    /DISCARD/ : { *(.eh_frame .eh_frame_hdr) }
}
