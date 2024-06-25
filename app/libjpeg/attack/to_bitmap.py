import json
from PIL import Image
import numpy as np
import sys

def load_json(filename):
    with open(filename, 'r') as file:
        data = json.load(file)
    return data

def invert_normalize_values(arr, new_min=0, new_max=255):
    arr = np.array(arr)
    old_min = arr.min()
    old_max = arr.max()
    
    if old_max == old_min:
        return np.full_like(arr, new_max)
    
    # Inverted normalization
    normalized = ((arr - old_min) / (old_max - old_min)) * (new_max - new_min) + new_min
    inverted_normalized = new_max - normalized + new_min
    return inverted_normalized.astype(np.uint8)

def adjust_channel(channel, factor=0.8):
    return np.clip(channel * factor, 0, 255).astype(np.uint8)

def ycbcr_to_rgb(y, cb, cr):
    # Convert YCbCr to RGB
    y = y.astype(np.float32)
    cb = cb.astype(np.float32) - 128
    cr = cr.astype(np.float32) - 128
    
    r = y + 1.402 * cr
    g = y - 0.344136 * cb - 0.714136 * cr
    b = y + 1.772 * cb
    
    # Clip values to be in the 0-255 range
    r = np.clip(r, 0, 255).astype(np.uint8)
    g = np.clip(g, 0, 255).astype(np.uint8)
    b = np.clip(b, 0, 255).astype(np.uint8)
    
    return np.stack([r, g, b], axis=-1)

def create_image(data, output_file):
    # Determine image dimensions
    num_layers = len(data)
    image_rows = len(data[0])
    image_cols = len(data[0][0])
    
    if num_layers == 1:
        # Grayscale image
        image_data = np.array(data[0])
        normalized_data = invert_normalize_values(image_data)
        img = Image.fromarray(normalized_data, 'L')
    elif num_layers == 3:
        # RGB image
        # r_data = adjust_channel(invert_normalize_values(np.array(data[0])), 1)
        # g_data = adjust_channel(invert_normalize_values(np.array(data[1])), 0.7)
        # b_data = adjust_channel(invert_normalize_values(np.array(data[2])), 0.5)

        # YCbCr image
        y_data = invert_normalize_values(np.array(data[0]))
        cb_data = invert_normalize_values(np.array(data[1]))
        cr_data = invert_normalize_values(np.array(data[2]))
        
        # Convert YCbCr to RGB
        rgb_data = ycbcr_to_rgb(y_data, cb_data, cr_data)

        # data_adjusted = adjust_channel(rgb_data[:, :, 0], factor=0.5)
        # rgb_data[:, :, 0] = data_adjusted
        # data_adjusted = adjust_channel(rgb_data[:, :, 1], factor=1)
        # rgb_data[:, :, 1] = data_adjusted
        # data_adjusted = adjust_channel(rgb_data[:, :, 2], factor=0.5)
        # rgb_data[:, :, 2] = data_adjusted
        
        img = Image.fromarray(rgb_data, 'RGB')
    else:
        raise ValueError("Unsupported color profile in JSON data.")
    
    img.save(output_file)
    print(f"Image saved as {output_file}")

def main():
    input_filename = sys.argv[1]    
    output_filename = sys.argv[2]    
    
    data = load_json(input_filename)
    create_image(data, output_filename)

if __name__ == "__main__":
    main()
