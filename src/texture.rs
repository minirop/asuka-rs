use crate::DxgiFormat;
use crate::Dds;
use crate::D3DFormat;
use serde::*;
use image_dds::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Texture {
    pub name: String,
    pub format: TextureFormat,
    pub filename: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Compression {
    Dxt1,
    Dxt3,
    Dxt5,
    A8R8G8B8,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum PixelFormat {
    Bgra8Unorm,
    BC1RgbaUnorm,
    BC1RgbaUnormSrgb,
    BC3RgbaUnorm,
    BC3RgbaUnormSrgb,
    BC7RgbaUnorm,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TextureFormat {
    D3DFormat(Compression, PixelFormat),
    DxgiFormat(PixelFormat),
}

fn d3d_to_compression(format: &D3DFormat) -> Compression {
    match format {
        D3DFormat::DXT1 => Compression::Dxt1,
        D3DFormat::DXT3 => Compression::Dxt3,
        D3DFormat::DXT5 => Compression::Dxt5,
        D3DFormat::A8R8G8B8 => Compression::A8R8G8B8,
        _ => panic!("Unhandled format: {:?}", format),
    }
}

fn compression_to_d3d(format: &Compression) -> D3DFormat {
    match format {
        Compression::Dxt1 => D3DFormat::DXT1,
        Compression::Dxt3 => D3DFormat::DXT5, // image_dds doesn't support DXT3
        Compression::Dxt5 => D3DFormat::DXT5,
        Compression::A8R8G8B8 => D3DFormat::A8R8G8B8,
    }
}

fn image_to_pixel_format(image: &ImageFormat) -> PixelFormat {
    match image {
        ImageFormat::BC1RgbaUnorm => PixelFormat::BC1RgbaUnorm,
        ImageFormat::BC3RgbaUnorm => PixelFormat::BC3RgbaUnorm,
        ImageFormat::Bgra8Unorm => PixelFormat::Bgra8Unorm,
        _ => panic!("Unsupported image format: {:?}", image),
    }
}

fn pixel_to_image_format(pixels: &PixelFormat) -> ImageFormat {
    match pixels {
        PixelFormat::BC1RgbaUnorm => ImageFormat::BC1RgbaUnorm,
        PixelFormat::BC3RgbaUnorm => ImageFormat::BC3RgbaUnorm,
        PixelFormat::Bgra8Unorm => ImageFormat::Bgra8Unorm,
        _ => panic!("Unsupported pixel format: {:?}", pixels),
    }
}

fn dxgi_to_texture_format(format: DxgiFormat) -> PixelFormat {
    match format {
        DxgiFormat::BC1_UNorm_sRGB => PixelFormat::BC1RgbaUnormSrgb,
        DxgiFormat::BC3_UNorm_sRGB => PixelFormat::BC3RgbaUnormSrgb,
        DxgiFormat::BC7_UNorm => PixelFormat::BC7RgbaUnorm,
        _ => panic!("Unsupported DXGI format: {:?}", format),
    }
}

#[allow(unused)]
fn texture_to_dxgi_format(format: TextureFormat) -> DxgiFormat {
    match format {
        _ => panic!("Unsupported Texture format: {:?}", format),
    }
}

pub fn dds_from_image(
    image: &image::RgbaImage,
    format: &TextureFormat,
) -> Result<Dds, CreateDdsError> {
    match format {
        TextureFormat::D3DFormat(compression, pixelformat) => {
            internal::d3d_from_image(image, compression, pixelformat)
        },
        TextureFormat::DxgiFormat(_pixelformat) => {
            todo!()
        },
    }
}

pub trait HeaderConverter {
    fn get_texture_format(&self) -> TextureFormat;
}

impl HeaderConverter for Dds {
    fn get_texture_format(&self) -> TextureFormat {
        if let Some(compression) = self.get_d3d_format() {
            let pixel = image_dds::dds_image_format(self).unwrap();
            TextureFormat::D3DFormat(
                d3d_to_compression(&compression),
                image_to_pixel_format(&pixel),
            )
        } else {
            TextureFormat::DxgiFormat(dxgi_to_texture_format(self.get_dxgi_format().unwrap()))
        }
    }
}

trait ToD3dDss {
    fn to_d3d_dds(&self, compression: &Compression) -> Result<image_dds::ddsfile::Dds, CreateDdsError>;
}

impl<T: AsRef<[u8]>> ToD3dDss for Surface<T> {
    fn to_d3d_dds(&self, compression: &Compression) -> Result<crate::ddsfile::Dds, CreateDdsError> {
        let mut dds = Dds::new_d3d(ddsfile::NewD3dParams {
            height: self.height,
            width: self.width,
            depth: None,
            format: compression_to_d3d(compression),
            mipmap_levels: None,
            caps2: None,
        })?;

        dds.data = self.data.as_ref().to_vec();

        Ok(dds)
    }
}

mod internal {

use super::PixelFormat;
use super::pixel_to_image_format;
use super::Compression;
use super::ToD3dDss;
use image_dds::*;
use ddsfile::Dds;

pub fn d3d_from_image(
    image: &image::RgbaImage,
    compression: &Compression,
    pixels: &PixelFormat,
) -> Result<Dds, CreateDdsError> {
    SurfaceRgba8::from_image(image)
        .encode(pixel_to_image_format(pixels), Quality::Normal, Mipmaps::Disabled)?
        .to_d3d_dds(compression)
}

}
