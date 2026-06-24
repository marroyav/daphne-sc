# RPU Hardware Backend Notes

This records the register contract the Rust RPU backend should implement. The
constants and packing helpers live in `daphne-sc-core/src/afe_hw.rs`.

## Sources

- `daphneZMQ/srcs/FpgaRegDict.cpp`
- `daphneZMQ/srcs/Afe.cpp`
- `daphneZMQ/srcs/Dac.cpp`
- `daphneZMQ/srcs/Dac.hpp`
- `daphne-firmware/ip_repo/daphne_ip/rtl/afe/spim_afe.vhd`
- `daphne-firmware/rtl/isolated/common/daphne_subsystem_pkg.vhd`
- `daphne-firmware/rtl/isolated/subsystems/analog/afe_config_slice.vhd`

## AFE Register Window

The current C++ server maps FPGA registers from base address `0x80000000`.
The AFE SPI window is:

- global control: offset `0x00`
- AFE control N: `0x04 + N * 0x0C`
- trim DAC N: `0x08 + N * 0x0C`
- offset DAC N: `0x0C + N * 0x0C`

The firmware `spim_afe.vhd` exposes the same offsets for the legacy AXI AFE
block. The isolated firmware path exposes equivalent per-AFE command records:
`afe_write_valid/data`, `trim_write_valid/data`, and `offset_write_valid/data`.

## AFE Register Sequence

C++ `Afe::setRegister()` writes:

1. `(register & 0xff) << 16 | (value & 0xffff)`
2. trigger word `0x000002`
3. address word `(register & 0xff) << 16`
4. read back low 16 bits
5. idle word `0x000000`

The RPU backend should perform the same sequence while polling the AFE busy
status. `Afe::getRegister()` uses the trigger word, then the address word, then
reads low 16 bits and clears the command register.

## DAC Words

Gain/bias DAC routing comes from `Dac.hpp`:

- AFE gain 0..3: `U50` channels 0..3
- AFE gain 4: `U5` channel 1
- AFE bias 0..3: `U53` channels 0..3
- AFE bias 4: `U5` channel 0
- VBIAS: `U5` channel 2

The gain/bias word is:

```text
(channel & 0x3) << 14 | gain << 13 | buffer << 12 | value & 0xfff
```

Trim/offset DAC writes use paired 16-bit halves. Channels 0..3 occupy the low
half, channels 4..7 occupy the high half, and companion channels differ by 4.
