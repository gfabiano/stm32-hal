//! Direct Memory Access (DMA). This module handles initialization, and transfer
//! configuration for DMA. The `Dma::cfg_channel` method is called by modules that use DMA.

use core::{
    ops::Deref,
    sync::atomic::{self, Ordering},
};

use crate::{
    pac::{self, RCC},
    rcc_en_reset,
};

#[cfg(feature = "g0")]
use crate::pac::dma;
#[cfg(not(feature = "g0"))]
use crate::pac::dma1 as dma;

// use embedded_dma::{ReadBuffer, WriteBuffer};

use cfg_if::cfg_if;

// todo: Several sections of this are only correct for DMA1.

#[derive(Copy, Clone)]
#[repr(u8)]
/// A list of DMA input sources. The integer values represent their DMAMUX register value, on
/// MCUs that use this. G4 RM, Table 91: DMAMUX: Assignment of multiplexer inputs to resources.
pub enum DmaInput {
    // This (on G4) goes up to 115. For now, just implement things we're likely
    // to use in this HAL. Make sure this is compatible beyond G4.
    Adc1 = 5,
    Dac1Ch1 = 6,
    Dac1Ch2 = 7,
    Tim6Up = 8,
    Tim7Up = 9,
    Spi1Rx = 10,
    Spi1Tx = 11,
    Spi2Rx = 12,
    Spi2Tx = 13,
    Spi3Rx = 14,
    Spi3Tx = 15,
    I2c1Rx = 16,
    I2c1Tx = 17,
    I2c2Rx = 18,
    I2c2Tx = 19,
    I2c3Rx = 20,
    I2c3Tx = 21,
    I2c4Rx = 22,
    I2c4Tx = 23,
    Usart1Rx = 24,
    Usart1Tx = 25,
    Usart2Rx = 26,
    Usart2Tx = 27,
    Usart3Rx = 28,
    Usart3Tx = 29,
    Uart4Rx = 30,
    Uart4Tx = 31,
    Uart5Rx = 32,
    Uart5Tx = 33,
    Lpuart1Rx = 34,
    Lpuart1Tx = 35,
    Adc2 = 36,
    Adc3 = 37,
    Adc4 = 38,
    Adc5 = 39,
}

impl DmaInput {
    #[cfg(any(feature = "f3", feature = "l4"))]
    /// Select the hard set channel associated with a given input source. See L44 RM, Table 41.
    pub fn dma1_channel(&self) -> DmaChannel {
        match self {
            Self::Adc1 => DmaChannel::C1,
            // Self::Dac1Ch1 => 6,
            // Self::Dac1Ch2 => 7,
            // Self::Tim6Up => 8,
            // Self::Tim7Up => 9,
            Self::Spi1Rx => DmaChannel::C2,
            Self::Spi1Tx => DmaChannel::C3,
            Self::Spi2Rx => DmaChannel::C4,
            Self::Spi2Tx => DmaChannel::C5,
            // Self::Spi3Rx => 14,
            // Self::Spi3Tx => 15,
            Self::I2c1Rx => DmaChannel::C7,
            Self::I2c1Tx => DmaChannel::C6,
            Self::I2c2Rx => DmaChannel::C5,
            Self::I2c2Tx => DmaChannel::C4,
            Self::I2c3Rx => DmaChannel::C3,
            // Self::I2c3Tx => 21,
            // Self::I2c4Rx => 22,
            // Self::I2c4Tx => 23,
            Self::Usart1Rx => DmaChannel::C5,
            Self::Usart1Tx => DmaChannel::C4,
            Self::Usart2Rx => DmaChannel::C6,
            Self::Usart2Tx => DmaChannel::C7,
            Self::Usart3Rx => DmaChannel::C3,
            Self::Usart3Tx => DmaChannel::C2,
            // Self::Uart4Rx => 30,
            // Self::Uart4Tx => 31,
            // Self::Uart5Rx => 32,
            // Self::Uart5Tx => 33,
            // Self::Lpuart1Rx => 34,
            // Self::Lpuart1Tx => 35,
            Self::Adc2 => DmaChannel::C2,
            // Self::Adc3 => 37,
            // Self::Adc4 => 38,
            // Self::Adc5 => 39,
            _ => unimplemented!(),
        }
    }

    #[cfg(feature = "l4")]
    /// Find the channel select value for a given DMA input. See L44 RM, Table 41.
    pub fn dma1_channel_select(&self) -> u8 {
        match self {
            Self::Adc1 => 0b000,
            // Self::Dac1Ch1 => 6,
            // Self::Dac1Ch2 => 7,
            // Self::Tim6Up => 8,
            // Self::Tim7Up => 9,
            Self::Spi1Rx => 0b001,
            Self::Spi1Tx => 0b001,
            Self::Spi2Rx => 0b001,
            Self::Spi2Tx => 0b001,
            // Self::Spi3Rx => 14,
            // Self::Spi3Tx => 15,
            Self::I2c1Rx => 0b011,
            Self::I2c1Tx => 0b011,
            Self::I2c2Rx => 0b011,
            Self::I2c2Tx => 0b011,
            Self::I2c3Rx => 0b011,
            // Self::I2c3Tx => 21,
            // Self::I2c4Rx => 22,
            // Self::I2c4Tx => 23,
            Self::Usart1Rx => 0b010,
            Self::Usart1Tx => 0b010,
            Self::Usart2Rx => 0b010,
            Self::Usart2Tx => 0b010,
            Self::Usart3Rx => 0b010,
            Self::Usart3Tx => 0b010,
            // Self::Uart4Rx => 30,
            // Self::Uart4Tx => 31,
            // Self::Uart5Rx => 32,
            // Self::Uart5Tx => 33,
            // Self::Lpuart1Rx => 34,
            // Self::Lpuart1Tx => 35,
            Self::Adc2 => 0b000,
            // Self::Adc3 => 37,
            // Self::Adc4 => 38,
            // Self::Adc5 => 39,
            _ => unimplemented!(),
        }
    }
}

#[derive(Copy, Clone)]
#[repr(u8)]
/// L4 RM, 11.4.3, "DMA arbitration":
/// The priorities are managed in two stages:
/// • software: priority of each channel is configured in the DMA_CCRx register, to one of
/// the four different levels:
/// – very high
/// – high
/// – medium
/// – low
/// • hardware: if two requests have the same software priority level, the channel with the
/// lowest index gets priority. For example, channel 2 gets priority over channel 4.
/// Only write to this when the channel is disabled.
pub enum Priority {
    Low = 0b00,
    Medium = 0b01,
    High = 0b10,
    VeryHigh = 0b11,
}

#[derive(Copy, Clone)]
/// Represents a DMA channel to select, eg when configuring for use with a peripheral.
pub enum DmaChannel {
    C1,
    C2,
    C3,
    C4,
    C5,
    // todo: Some G0 variants have channels 6 and 7 and DMA1. (And up to 5 channels on DMA2)
    #[cfg(not(feature = "g0"))]
    C6,
    #[cfg(not(feature = "g0"))]
    C7,
    // todo: Which else have 8? Also, note that some have diff amoutns on dam1 vs 2.
    #[cfg(any(feature = "l5", feature = "g4"))]
    C8,
}

#[derive(Copy, Clone)]
#[repr(u8)]
/// Set in CCR.
/// Can only be set when channel is disabled.
pub enum Direction {
    /// DIR = 0 defines typically a peripheral-to-memory transfer
    ReadFromPeriph = 0,
    /// DIR = 1 defines typically a memory-to-peripheral transfer.
    ReadFromMem = 1,
}

#[derive(Copy, Clone)]
#[repr(u8)]
/// Set in CCR.
/// Can only be set when channel is disabled.
pub enum Circular {
    Disabled = 0,
    Enabled = 1,
}

#[derive(Copy, Clone)]
#[repr(u8)]
/// Peripheral and memory increment mode. (CCR PINC and MINC bits)
/// Can only be set when channel is disabled.
pub enum IncrMode {
    // Can only be set when channel is disabled.
    Disabled = 0,
    Enabled = 1,
}

#[derive(Copy, Clone)]
#[repr(u8)]
/// Peripheral and memory increment mode. (CCR PSIZE and MSIZE bits)
/// Can only be set when channel is disabled.
pub enum DataSize {
    S8 = 0b00, // ie 8 bits
    S16 = 0b01,
    S32 = 0b10,
}

#[derive(Copy, Clone)]
/// Interrupt type. Set in CCR using TEIE, HTIE, and TCIE bits.
/// Can only be set when channel is disabled.
pub enum DmaInterrupt {
    TransferError,
    HalfTransfer,
    TransferComplete,
}

/// Reduce DRY over channels when configuring a channel's CCR.
/// We must use a macro here, since match arms balk at the incompatible
/// types of `CCR1`, `CCR2` etc.
macro_rules! set_ccr {
    ($ccr:expr, $priority:expr, $direction:expr, $circular:expr, $periph_incr:expr, $mem_incr:expr, $periph_size:expr, $mem_size:expr) => {
        // "The register fields/bits MEM2MEM, PL[1:0], MSIZE[1:0], PSIZE[1:0], MINC, PINC, and DIR
        // are read-only when EN = 1"
        $ccr.modify(|_, w| w.en().clear_bit());
        while $ccr.read().en().bit_is_set() {}

        if let Circular::Enabled = $circular {
            $ccr.modify(|_, w| w.mem2mem().clear_bit());
        }

        $ccr.modify(|_, w| unsafe {
            // – the channel priority
            w.pl().bits($priority as u8);
            // – the data transfer direction
            // This bit [DIR] must be set only in memory-to-peripheral and peripheral-to-memory modes.
            // 0: read from peripheral
            w.dir().bit($direction as u8 != 0);
            // – the circular mode
            w.circ().bit($circular as u8 != 0);
            // – the peripheral and memory incremented mode
            w.pinc().bit($periph_incr as u8 != 0);
            w.minc().bit($mem_incr as u8 != 0);
            // – the peripheral and memory data size
            w.psize().bits($periph_size as u8);
            w.msize().bits($mem_size as u8);
            // – the interrupt enable at half and/or full transfer and/or transfer error
            w.tcie().set_bit();
            // (See `Step 5` above.)
            w.en().set_bit()
        });
    }
}

/// Reduce DRY over channels when configuring a channel's interrupts.
macro_rules! enable_interrupt {
    ($ccr:expr, $interrupt_type:expr) => {
        let originally_enabled = $ccr.read().en().bit_is_set();
        if originally_enabled {
            $ccr.modify(|_, w| w.en().clear_bit());
            while $ccr.read().en().bit_is_set() {}
        }
        match $interrupt_type {
            DmaInterrupt::TransferError => $ccr.modify(|_, w| w.teie().set_bit()),
            DmaInterrupt::HalfTransfer => $ccr.modify(|_, w| w.htie().set_bit()),
            DmaInterrupt::TransferComplete => $ccr.modify(|_, w| w.tcie().set_bit()),
        }

        if originally_enabled {
            $ccr.modify(|_, w| w.en().set_bit());
            while $ccr.read().en().bit_is_clear() {}
        }
    };
}

/// This struct is used to pass common (non-peripheral and non-use-specific) data when configuring
/// a channel.
pub struct ChannelCfg {
    priority: Priority,
    circular: Circular,
    periph_incr: IncrMode,
    mem_incr: IncrMode,
}

impl Default for ChannelCfg {
    fn default() -> Self {
        Self {
            priority: Priority::Medium,   // todo: Pass pri as an arg?
            circular: Circular::Disabled, // todo?
            // Increment the buffer address, not the peripheral address.
            periph_incr: IncrMode::Disabled,
            mem_incr: IncrMode::Enabled,
        }
    }
}

/// Represents a Direct Memory Access (DMA) peripheral.
pub struct Dma<D> {
    regs: D,
}

impl<D> Dma<D>
where
    D: Deref<Target = dma::RegisterBlock>,
{
    pub fn new(regs: D, rcc: &mut RCC) -> Self {
        // todo: Enable RCC for DMA 2 etc!

        cfg_if! {
            if #[cfg(feature = "f3")] {
                rcc.ahbenr.modify(|_, w| w.dma1en().set_bit()); // no dmarst on F3.
            } else if #[cfg(feature = "g0")] {
                rcc_en_reset!(ahb1, dma, rcc);
            } else {
                rcc_en_reset!(ahb1, dma1, rcc);
            }
        }

        Self { regs }
    }

    /// Configure a DMA channel. See L4 RM 0394, section 11.4.4. Sets the Transfer Complete
    /// interrupt.
    pub fn cfg_channel(
        &mut self,
        channel: DmaChannel,
        periph_addr: u32,
        mem_addr: u32,
        num_data: u16,
        direction: Direction,
        periph_size: DataSize,
        mem_size: DataSize,
        cfg: ChannelCfg,
    ) {
        // The following sequence is needed to configure a DMA channel x:
        // 1. Set the peripheral register address in the DMA_CPARx register.
        // The data is moved from/to this address to/from the memory after the peripheral event,
        // or after the channel is enabled in memory-to-memory mode.

        unsafe {
            match channel {
                DmaChannel::C1 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cpar = &self.regs.ch1.par;
                        } else {
                            let cpar = &self.regs.cpar1;
                        }
                    }
                    cpar.write(|w| w.bits(periph_addr));
                }
                DmaChannel::C2 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cpar = &self.regs.ch2.par;
                        } else {
                            let cpar = &self.regs.cpar2;
                        }
                    }
                    cpar.write(|w| w.bits(periph_addr));
                }
                DmaChannel::C3 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cpar = &self.regs.ch3.par;
                        } else {
                            let cpar = &self.regs.cpar3;
                        }
                    }
                    cpar.write(|w| w.bits(periph_addr));
                }
                DmaChannel::C4 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cpar = &self.regs.ch4.par;
                        } else {
                            let cpar = &self.regs.cpar4;
                        }
                    }
                    cpar.write(|w| w.bits(periph_addr));
                }
                DmaChannel::C5 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cpar = &self.regs.ch5.par;
                        } else {
                            let cpar = &self.regs.cpar5;
                        }
                    }
                    cpar.write(|w| w.bits(periph_addr));
                }
                #[cfg(not(feature = "g0"))]
                DmaChannel::C6 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cpar = &self.regs.ch6.par;
                        } else {
                            let cpar = &self.regs.cpar6;
                        }
                    }
                    cpar.write(|w| w.bits(periph_addr));
                }
                #[cfg(not(feature = "g0"))]
                DmaChannel::C7 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cpar = &self.regs.ch7.par;
                        } else {
                            let cpar = &self.regs.cpar7;
                        }
                    }
                    cpar.write(|w| w.bits(periph_addr));
                }
                #[cfg(any(feature = "l5", feature = "g4"))]
                DmaChannel::C8 => {
                    let cpar = &self.regs.cpar8;
                    cpar.write(|w| w.bits(periph_addr));
                }
            }
        }

        // 2. Set the memory address in the DMA_CMARx register.
        // The data is written to/read from the memory after the peripheral event or after the
        // channel is enabled in memory-to-memory mode.
        unsafe {
            match channel {
                DmaChannel::C1 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cmar = &self.regs.ch1.mar;
                        } else {
                            let cmar = &self.regs.cmar1;
                        }
                    }
                    cmar.write(|w| w.bits(mem_addr));
                }
                DmaChannel::C2 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cmar = &self.regs.ch2.mar;
                        } else {
                            let cmar = &self.regs.cmar2;
                        }
                    }
                    cmar.write(|w| w.bits(mem_addr));
                }
                DmaChannel::C3 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cmar = &self.regs.ch3.mar;
                        } else {
                            let cmar = &self.regs.cmar3;
                        }
                    }
                    cmar.write(|w| w.bits(mem_addr));
                }
                DmaChannel::C4 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cmar = &self.regs.ch4.mar;
                        } else {
                            let cmar = &self.regs.cmar4;
                        }
                    }
                    cmar.write(|w| w.bits(mem_addr));
                }
                DmaChannel::C5 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cmar = &self.regs.ch5.mar;
                        } else {
                            let cmar = &self.regs.cmar5;
                        }
                    }
                    cmar.write(|w| w.bits(mem_addr));
                }
                #[cfg(not(feature = "g0"))]
                DmaChannel::C6 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cmar = &self.regs.ch6.mar;
                        } else {
                            let cmar = &self.regs.cmar6;
                        }
                    }
                    cmar.write(|w| w.bits(mem_addr));
                }
                #[cfg(not(feature = "g0"))]
                DmaChannel::C7 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cmar = &self.regs.ch7.mar;
                        } else {
                            let cmar = &self.regs.cmar7;
                        }
                    }
                    cmar.write(|w| w.bits(mem_addr));
                }
                #[cfg(any(feature = "l5", feature = "g4"))]
                DmaChannel::C8 => {
                    let cmar = &self.regs.cmar8;
                    cmar.write(|w| w.bits(mem_addr));
                }
            }
        }

        // 3. Configure the total number of data to transfer in the DMA_CNDTRx register.
        // After each data transfer, this value is decremented.
        unsafe {
            match channel {
                DmaChannel::C1 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cndtr = &self.regs.ch1.ndtr;
                        } else {
                            let cndtr = &self.regs.cndtr1;
                        }
                    }
                    cndtr.write(|w| w.ndt().bits(num_data));
                }
                DmaChannel::C2 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cndtr = &self.regs.ch2.ndtr;
                        } else {
                            let cndtr = &self.regs.cndtr2;
                        }
                    }
                    cndtr.write(|w| w.ndt().bits(num_data));
                }
                DmaChannel::C3 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cndtr = &self.regs.ch3.ndtr;
                        } else {
                            let cndtr = &self.regs.cndtr3;
                        }
                    }
                    cndtr.write(|w| w.ndt().bits(num_data));
                }
                DmaChannel::C4 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cndtr = &self.regs.ch4.ndtr;
                        } else {
                            let cndtr = &self.regs.cndtr4;
                        }
                    }
                    cndtr.write(|w| w.ndt().bits(num_data));
                }
                DmaChannel::C5 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cndtr = &self.regs.ch5.ndtr;
                        } else {
                            let cndtr = &self.regs.cndtr5;
                        }
                    }
                    cndtr.write(|w| w.ndt().bits(num_data));
                }
                #[cfg(not(feature = "g0"))]
                DmaChannel::C6 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cndtr = &self.regs.ch6.ndtr;
                        } else {
                            let cndtr = &self.regs.cndtr6;
                        }
                    }
                    cndtr.write(|w| w.ndt().bits(num_data));
                }
                #[cfg(not(feature = "g0"))]
                DmaChannel::C7 => {
                    cfg_if! {
                        if #[cfg(any(feature = "f3", feature = "g0"))] {
                            let cndtr = &self.regs.ch7.ndtr;
                        } else {
                            let cndtr = &self.regs.cndtr7;
                        }
                    }
                    cndtr.write(|w| w.ndt().bits(num_data));
                }
                #[cfg(any(feature = "l5", feature = "g4"))]
                DmaChannel::C8 => {
                    let cndtr = &self.regs.cndtr8;
                    cndtr.write(|w| w.ndt().bits(num_data));
                }
            }
        }

        // 4. Configure the parameters listed below in the DMA_CCRx register:
        // (These are listed below by their corresponding reg write code)

        // todo: See note about sep reg writes to disable channel, and when you need to do this.

        // 5. Activate the channel by setting the EN bit in the DMA_CCRx register.
        // A channel, as soon as enabled, may serve any DMA request from the peripheral connected
        // to this channel, or may start a memory-to-memory block transfer.
        // Note: The two last steps of the channel configuration procedure may be merged into a single
        // access to the DMA_CCRx register, to configure and enable the channel.
        // When a channel is enabled and still active (not completed), the software must perform two
        // separate write accesses to the DMA_CCRx register, to disable the channel, then to
        // reprogram the channel for another next block transfer.
        // Some fields of the DMA_CCRx register are read-only when the EN bit is set to 1

        // (later): The circular mode must not be used in memory-to-memory mode. Before enabling a
        // channel in circular mode (CIRC = 1), the software must clear the MEM2MEM bit of the
        // DMA_CCRx register. When the circular mode is activated, the amount of data to transfer is
        // automatically reloaded with the initial value programmed during the channel configuration
        // phase, and the DMA requests continue to be served

        // (See remainder of steps in `set_ccr()!` macro.

        // todo: Let user set mem2mem mode?

        // See the [Embedonomicon section on DMA](https://docs.rust-embedded.org/embedonomicon/dma.html)
        // for info on why we use `compiler_fence` here:
        // "We use Ordering::Release to prevent all preceding memory operations from being moved
        // after [starting DMA], which performs a volatile write."
        atomic::compiler_fence(Ordering::Release);

        match channel {
            DmaChannel::C1 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch1.cr;
                    } else {
                        let ccr = &self.regs.ccr1;
                    }
                }
                set_ccr!(
                    ccr,
                    cfg.priority,
                    direction,
                    cfg.circular,
                    cfg.periph_incr,
                    cfg.mem_incr,
                    periph_size,
                    mem_size
                );
            }
            DmaChannel::C2 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch2.cr;
                    } else {
                        let ccr = &self.regs.ccr2;
                    }
                }
                set_ccr!(
                    ccr,
                    cfg.priority,
                    direction,
                    cfg.circular,
                    cfg.periph_incr,
                    cfg.mem_incr,
                    periph_size,
                    mem_size
                );
            }
            DmaChannel::C3 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch3.cr;
                    } else {
                        let ccr = &self.regs.ccr3;
                    }
                }
                set_ccr!(
                    ccr,
                    cfg.priority,
                    direction,
                    cfg.circular,
                    cfg.periph_incr,
                    cfg.mem_incr,
                    periph_size,
                    mem_size
                );
            }
            DmaChannel::C4 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch4.cr;
                    } else {
                        let ccr = &self.regs.ccr4;
                    }
                }
                set_ccr!(
                    ccr,
                    cfg.priority,
                    direction,
                    cfg.circular,
                    cfg.periph_incr,
                    cfg.mem_incr,
                    periph_size,
                    mem_size
                );
            }
            DmaChannel::C5 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch5.cr;
                    } else {
                        let ccr = &self.regs.ccr5;
                    }
                }
                set_ccr!(
                    ccr,
                    cfg.priority,
                    direction,
                    cfg.circular,
                    cfg.periph_incr,
                    cfg.mem_incr,
                    periph_size,
                    mem_size
                );
            }
            #[cfg(not(feature = "g0"))]
            DmaChannel::C6 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch6.cr;
                    } else {
                        let ccr = &self.regs.ccr6;
                    }
                }
                set_ccr!(
                    ccr,
                    cfg.priority,
                    direction,
                    cfg.circular,
                    cfg.periph_incr,
                    cfg.mem_incr,
                    periph_size,
                    mem_size
                );
            }
            #[cfg(not(feature = "g0"))]
            DmaChannel::C7 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch7.cr;
                    } else {
                        let ccr = &self.regs.ccr7;
                    }
                }
                set_ccr!(
                    ccr,
                    cfg.priority,
                    direction,
                    cfg.circular,
                    cfg.periph_incr,
                    cfg.mem_incr,
                    periph_size,
                    mem_size
                );
            }
            #[cfg(any(feature = "l5", feature = "g4"))]
            DmaChannel::C8 => {
                let mut ccr = &self.regs.ccr8;
                set_ccr!(
                    ccr,
                    cfg.priority,
                    direction,
                    cfg.circular,
                    cfg.periph_incr,
                    cfg.mem_incr,
                    periph_size,
                    mem_size
                );
            }
        }
    }

    pub fn stop(&mut self, channel: DmaChannel) {
        // L4 RM:
        // Once the software activates a channel, it waits for the completion of the programmed
        // transfer. The DMA controller is not able to resume an aborted active channel with a possible
        // suspended bus transfer.
        // To correctly stop and disable a channel, the software clears the EN bit of the DMA_CCRx
        // register.

        match channel {
            DmaChannel::C1 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch1.cr;
                    } else {
                        let ccr = &self.regs.ccr1;
                    }
                }
                ccr.modify(|_, w| w.en().clear_bit());
                while ccr.read().en().bit_is_set() {}
            }
            DmaChannel::C2 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch2.cr;
                    } else {
                        let ccr = &self.regs.ccr2;
                    }
                }
                ccr.modify(|_, w| w.en().clear_bit());
                while ccr.read().en().bit_is_set() {}
            }
            DmaChannel::C3 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch3.cr;
                    } else {
                        let ccr = &self.regs.ccr3;
                    }
                }
                ccr.modify(|_, w| w.en().clear_bit());
                while ccr.read().en().bit_is_set() {}
            }
            DmaChannel::C4 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch4.cr;
                    } else {
                        let ccr = &self.regs.ccr4;
                    }
                }
                ccr.modify(|_, w| w.en().clear_bit());
                while ccr.read().en().bit_is_set() {}
            }
            DmaChannel::C5 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch5.cr;
                    } else {
                        let ccr = &self.regs.ccr5;
                    }
                }
                ccr.modify(|_, w| w.en().clear_bit());
                while ccr.read().en().bit_is_set() {}
            }
            #[cfg(not(feature = "g0"))]
            DmaChannel::C6 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch6.cr;
                    } else {
                        let ccr = &self.regs.ccr6;
                    }
                }
                ccr.modify(|_, w| w.en().clear_bit());
                while ccr.read().en().bit_is_set() {}
            }
            #[cfg(not(feature = "g0"))]
            DmaChannel::C7 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch7.cr;
                    } else {
                        let ccr = &self.regs.ccr7;
                    }
                }
                ccr.modify(|_, w| w.en().clear_bit());
                while ccr.read().en().bit_is_set() {}
            }
            #[cfg(any(feature = "l5", feature = "g4"))]
            DmaChannel::C8 => {
                let ccr = &self.regs.ccr8;
                ccr.modify(|_, w| w.en().clear_bit());
            }
        };

        // The software secures that no pending request from the peripheral is served by the
        // DMA controller before the transfer completion.
        // todo?

        // The software waits for the transfer complete or transfer error interrupt.
        // (Handed by calling code)

        // (todo: set ifcr.cficx bit to clear all interrupts?)

        // When a channel transfer error occurs, the EN bit of the DMA_CCRx register is cleared by
        // hardware. This EN bit can not be set again by software to re-activate the channel x, until the
        // TEIFx bit of the DMA_ISR register is set
        atomic::compiler_fence(Ordering::SeqCst);
    }

    pub fn transfer_is_complete(&mut self, channel: DmaChannel) -> bool {
        let isr_val = self.regs.isr.read();
        match channel {
            DmaChannel::C1 => isr_val.tcif1().bit_is_set(),
            DmaChannel::C2 => isr_val.tcif2().bit_is_set(),
            DmaChannel::C3 => isr_val.tcif3().bit_is_set(),
            DmaChannel::C4 => isr_val.tcif4().bit_is_set(),
            DmaChannel::C5 => isr_val.tcif5().bit_is_set(),
            #[cfg(not(feature = "g0"))]
            DmaChannel::C6 => isr_val.tcif6().bit_is_set(),
            #[cfg(not(feature = "g0"))]
            DmaChannel::C7 => isr_val.tcif7().bit_is_set(),
            #[cfg(any(feature = "l5", feature = "g4"))]
            DmaChannel::C8 => isr_val.tcif8().bit_is_set(),
        }
    }

    #[cfg(feature = "l4")] // Only required on L4
    /// Select which peripheral on a given channel we're using.
    /// See L44 RM, Table 41.
    pub fn channel_select(&mut self, input: DmaInput) {
        // todo: Allow selecting channels in pairs to save a write.
        let val = input.dma1_channel_select();
        match input.dma1_channel() {
            DmaChannel::C1 => self.regs.cselr.modify(|_, w| w.c1s().bits(val)),
            DmaChannel::C2 => self.regs.cselr.modify(|_, w| w.c2s().bits(val)),
            DmaChannel::C3 => self.regs.cselr.modify(|_, w| w.c3s().bits(val)),
            DmaChannel::C4 => self.regs.cselr.modify(|_, w| w.c4s().bits(val)),
            DmaChannel::C5 => self.regs.cselr.modify(|_, w| w.c5s().bits(val)),
            DmaChannel::C6 => self.regs.cselr.modify(|_, w| w.c6s().bits(val)),
            DmaChannel::C7 => self.regs.cselr.modify(|_, w| w.c7s().bits(val)),
        }
    }

    /// Enable a specific type of interrupt. Note that the `TransferComplete` interrupt
    /// is enabled automatically, by the `cfg_channel` method.
    pub fn enable_interrupt(&mut self, channel: DmaChannel, interrupt: DmaInterrupt) {
        // Can only be set when the channel is disabled.
        match channel {
            DmaChannel::C1 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch1.cr;
                    } else {
                        let ccr = &self.regs.ccr1;
                    }
                }
                enable_interrupt!(ccr, interrupt);
            }
            DmaChannel::C2 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch2.cr;
                    } else {
                        let ccr = &self.regs.ccr2;
                    }
                }
                enable_interrupt!(ccr, interrupt);
            }
            DmaChannel::C3 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch3.cr;
                    } else {
                        let ccr = &self.regs.ccr3;
                    }
                }
                enable_interrupt!(ccr, interrupt);
            }
            DmaChannel::C4 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch4.cr;
                    } else {
                        let ccr = &self.regs.ccr4;
                    }
                }
                enable_interrupt!(ccr, interrupt);
            }
            DmaChannel::C5 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch5.cr;
                    } else {
                        let ccr = &self.regs.ccr5;
                    }
                }
                enable_interrupt!(ccr, interrupt);
            }
            #[cfg(not(feature = "g0"))]
            DmaChannel::C6 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch6.cr;
                    } else {
                        let ccr = &self.regs.ccr6;
                    }
                }
                enable_interrupt!(ccr, interrupt);
            }
            #[cfg(not(feature = "g0"))]
            DmaChannel::C7 => {
                cfg_if! {
                    if #[cfg(any(feature = "f3", feature = "g0"))] {
                        let ccr = &self.regs.ch7.cr;
                    } else {
                        let ccr = &self.regs.ccr7;
                    }
                }
                enable_interrupt!(ccr, interrupt);
            }
            #[cfg(any(feature = "l5", feature = "g4"))]
            DmaChannel::C8 => {
                let ccr = &self.regs.ccr8;
                enable_interrupt!(ccr, interrupt);
            }
        };
    }

    pub fn clear_interrupt(&mut self, channel: DmaChannel, interrupt: DmaInterrupt) {
        cfg_if! {
            if #[cfg(feature = "g4")] {
                match channel {
                    DmaChannel::C1 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.teif1().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.htif1().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.tcif1().set_bit()),
                    }
                    DmaChannel::C2 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.teif2().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.htif2().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.tcif2().set_bit()),
                    }
                    DmaChannel::C3 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.teif3().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.htif3().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.tcif3().set_bit()),
                    }
                    DmaChannel::C4 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.teif4().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.htif4().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.tcif4().set_bit()),
                    }
                    DmaChannel::C5 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.teif5().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.htif5().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.tcif5().set_bit()),
                    }
                    DmaChannel::C6 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.teif6().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.htif6().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.tcif6().set_bit()),
                    }
                    DmaChannel::C7 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.teif7().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.htif7().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.tcif7().set_bit()),
                    }
                    DmaChannel::C8 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.teif8().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.htif8().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.tcif8().set_bit()),
                    }
                }
            } else {
                match channel {
                    DmaChannel::C1 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.cteif1().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.chtif1().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.ctcif1().set_bit()),
                    }
                    DmaChannel::C2 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.cteif2().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.chtif2().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.ctcif2().set_bit()),
                    }
                    DmaChannel::C3 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.cteif3().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.chtif3().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.ctcif3().set_bit()),
                    }
                    DmaChannel::C4 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.cteif4().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.chtif4().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.ctcif4().set_bit()),
                    }
                    DmaChannel::C5 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.cteif5().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.chtif5().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.ctcif5().set_bit()),
                    }
                    #[cfg(not(feature = "g0"))]
                    DmaChannel::C6 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.cteif6().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.chtif6().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.ctcif6().set_bit()),
                    }
                    #[cfg(not(feature = "g0"))]
                    DmaChannel::C7 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.cteif7().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.chtif7().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.ctcif7().set_bit()),
                    }
                    #[cfg(any(feature = "l5", feature = "g4"))]
                    DmaChannel::C8 => match interrupt {
                        DmaInterrupt::TransferError => self.regs.ifcr.write(|w| w.cteif8().set_bit()),
                        DmaInterrupt::HalfTransfer => self.regs.ifcr.write(|w| w.chtif8().set_bit()),
                        DmaInterrupt::TransferComplete => self.regs.ifcr.write(|w| w.ctcif8().set_bit()),
                    }
                }
            }
        }
    }
}

// // todo: Remove the static reqs once you get thi sworking.
// // todo: If you end up using these, move to util.
// // todo: Set up a global flag to figure out if this is in use to prevent concurrent SPI
// // todo activity while in use??
// // todo: Impl Drop for DmaWriteBuf, where it stops the transfer.
// pub struct DmaWriteBuf<'a, T> {
//     // pub buf: &'static [u8]
//     pub buf: &'a mut [T], // pub channel: DmaChannel,
//
//     // #[repr(align(4))]
//     // struct Aligned<T: ?Sized>(T);
//     //s tatic mut BUF: Aligned<[u16; 8]> = Aligned([0; 8]);
// }
//
// // unsafe impl StaticWriteBuffer for DmaWriteBuf {
// //     type Word = u8;
// //
// //     unsafe fn static_write_buffer(&mut self) -> (*mut Self::Word, usize) {
// //         (self.buf.as_mut_ptr(), self.buf.len())
// //     }
// // }
//
// unsafe impl<'a, T> WriteBuffer for DmaWriteBuf<'a, T> {
//     type Word = T;
//
//     unsafe fn write_buffer(&mut self) -> (*mut Self::Word, usize) {
//         (self.buf.as_mut_ptr(), self.buf.len())
//     }
// }
//
// impl<T> Drop for DmaWriteBuf<'_, T> {
//     // todo: Hardcoded for DMA1 and Chan 3.
//     // todo: Does this stop all transfers in progress?
//     fn drop(&mut self) {
//         unsafe {
//             cfg_if! {
//                 if #[cfg(feature = "g4")] {
//                     (*pac::DMA1::ptr()).ifcr.write(|w| w.gif2().clear_bit());
//                 } else if #[cfg(feature = "g0")] {
//                 } else if #[cfg(feature = "g0")] {
//                     (*pac::DMA::ptr()).ifcr.write(|w| w.cgif2().clear_bit());
//                 } else {
//                     (*pac::DMA1::ptr()).ifcr.write(|w| w.cgif2().clear_bit());
//                 }
//             }
//             cfg_if! {
//                 if #[cfg(feature = "f3")] {
//                     (*pac::DMA1::ptr()).ch2.cr.modify(|_, w| w.en().clear_bit());
//                 } else if #[cfg(feature = "g0")] {
//                     (*pac::DMA::ptr()).ch2.cr.modify(|_, w| w.en().clear_bit());
//                 } else {
//                     (*pac::DMA1::ptr()).ccr2.modify(|_, w| w.en().clear_bit());
//                 }
//             }
//         }
//     }
// }
//
// pub struct DmaReadBuf<'a, T> {
//     // pub buf: &'static [u8]
//     pub buf: &'a [T],
// }
//
// // unsafe impl StaticReadBuffer for DmaReadBuf {
// //     type Word = u8;
// //
// //     unsafe fn static_write_buffer(&self) -> (*const Self::Word, usize) {
// //         (self.buf[.as_ptr(), self.buf.len())
// //     }
// // }
//
// unsafe impl<'a, T> ReadBuffer for DmaReadBuf<'a, T> {
//     type Word = T;
//
//     unsafe fn read_buffer(&self) -> (*const Self::Word, usize) {
//         (self.buf.as_ptr(), self.buf.len())
//     }
// }
//
// impl<T> Drop for DmaReadBuf<'_, T> {
//     // todo: Hardcoded for DMA1 and Chan 2.
//     // todo: Does this stop all transfers in progress?
//
//     // todo: DRY with impl in DmaWriteBuf above.
//     fn drop(&mut self) {
//         unsafe {
//             // Global interrupt clear flag for this channel.
//             cfg_if! {
//                 if #[cfg(feature = "g4")] {
//                     (*pac::DMA1::ptr()).ifcr.write(|w| w.gif2().clear_bit());
//                 } else if #[cfg(feature = "g0")] {
//                     (*pac::DMA::ptr()).ifcr.write(|w| w.cgif2().clear_bit());
//                 } else {
//                     (*pac::DMA1::ptr()).ifcr.write(|w| w.cgif2().clear_bit());
//                 }
//             }
//             cfg_if! {
//                 if #[cfg(feature = "f3")] {
//                     (*pac::DMA1::ptr()).ch2.cr.modify(|_, w| w.en().clear_bit());
//                 } else if #[cfg(feature = "g0")] {
//                     (*pac::DMA::ptr()).ch2.cr.modify(|_, w| w.en().clear_bit());
//                 } else {
//                     (*pac::DMA1::ptr()).ccr2.modify(|_, w| w.en().clear_bit());
//                 }
//             }
//         }
//     }
// }

#[cfg(any(feature = "l5", feature = "g0", feature = "g4", feature = "wb"))]
/// Configure a specific DMA channel to work with a specific peripheral.
pub fn mux(channel: DmaChannel, input: DmaInput, mux: &pac::DMAMUX) {
    // Note: This is similar in API and purpose to `channel_select` above,
    // for different families. We're keeping it as a separate function instead
    // of feature-gating within the same function so the name can be recognizable
    // from the RM etc.
    unsafe {
        #[cfg(not(any(feature = "g070", feature = "g071", feature = "g081")))]
        match channel {
            DmaChannel::C1 => mux.c1cr.modify(|_, w| w.dmareq_id().bits(input as u8)),
            DmaChannel::C2 => mux.c2cr.modify(|_, w| w.dmareq_id().bits(input as u8)),
            DmaChannel::C3 => mux.c3cr.modify(|_, w| w.dmareq_id().bits(input as u8)),
            DmaChannel::C4 => mux.c4cr.modify(|_, w| w.dmareq_id().bits(input as u8)),
            DmaChannel::C5 => mux.c5cr.modify(|_, w| w.dmareq_id().bits(input as u8)),
            #[cfg(not(feature = "g0"))]
            DmaChannel::C6 => mux.c6cr.modify(|_, w| w.dmareq_id().bits(input as u8)),
            #[cfg(not(feature = "g0"))]
            DmaChannel::C7 => mux.c7cr.modify(|_, w| w.dmareq_id().bits(input as u8)),
            #[cfg(any(feature = "l5", feature = "g4"))]
            DmaChannel::C8 => mux.c8cr.modify(|_, w| w.dmareq_id().bits(input as u8)),
        }
        #[cfg(any(feature = "g070", feature = "g071", feature = "g081"))]
        match channel {
            DmaChannel::C1 => mux
                .dmamux_c1cr
                .modify(|_, w| w.dmareq_id().bits(input as u8)),
            DmaChannel::C2 => mux
                .dmamux_c2cr
                .modify(|_, w| w.dmareq_id().bits(input as u8)),
            DmaChannel::C3 => mux
                .dmamux_c3cr
                .modify(|_, w| w.dmareq_id().bits(input as u8)),
            DmaChannel::C4 => mux
                .dmamux_c4cr
                .modify(|_, w| w.dmareq_id().bits(input as u8)),
            DmaChannel::C5 => mux
                .dmamux_c5cr
                .modify(|_, w| w.dmareq_id().bits(input as u8)),
        }
    }
}
