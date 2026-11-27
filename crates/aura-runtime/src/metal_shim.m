// The Objective-C side of the `tensor` module's Metal device. See
// `tensor_metal.rs`.
//
// Nothing here names a framework class at link time. The frameworks are
// opened and the classes are looked up when a program first asks for the
// device, so an Aura program that never does links and loads none of
// them.

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <MetalPerformanceShadersGraph/MetalPerformanceShadersGraph.h>
#include <dlfcn.h>
#include <objc/runtime.h>
#include <pthread.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

static id<MTLDevice> aura_device;
static id<MTLCommandQueue> aura_queue;
static Class aura_graph_class;
static Class aura_data_class;
static Class aura_number_class;
static Class aura_array_class;
static Class aura_dictionary_class;
static Class aura_bytes_class;
static int aura_ready;
static int aura_indexed;

typedef id<MTLDevice> (*AuraCreateDevice)(void) NS_RETURNS_RETAINED;

static void aura_metal_load(void) {
    @autoreleasepool {
        void *metal = dlopen("/System/Library/Frameworks/Metal.framework/Metal", RTLD_LAZY | RTLD_LOCAL);
        void *graph = dlopen(
            "/System/Library/Frameworks/MetalPerformanceShadersGraph.framework/MetalPerformanceShadersGraph",
            RTLD_LAZY | RTLD_LOCAL);
        if (metal == NULL || graph == NULL) {
            if (metal != NULL) {
                dlclose(metal);
            }
            if (graph != NULL) {
                dlclose(graph);
            }
            return;
        }
        AuraCreateDevice create = (AuraCreateDevice)dlsym(metal, "MTLCreateSystemDefaultDevice");
        if (create == NULL) {
            return;
        }
        aura_device = create();
        if (aura_device == nil) {
            return;
        }
        aura_queue = [aura_device newCommandQueue];
        aura_graph_class = objc_getClass("MPSGraph");
        aura_data_class = objc_getClass("MPSGraphTensorData");
        aura_number_class = objc_getClass("NSNumber");
        aura_array_class = objc_getClass("NSMutableArray");
        aura_dictionary_class = objc_getClass("NSMutableDictionary");
        aura_bytes_class = objc_getClass("NSData");
        if (aura_queue == nil || aura_graph_class == Nil || aura_data_class == Nil ||
            aura_number_class == Nil || aura_array_class == Nil || aura_dictionary_class == Nil ||
            aura_bytes_class == Nil) {
            return;
        }
        // A virtual machine can name a device whose allocations fail.
        id<MTLBuffer> probe = [aura_device newBufferWithLength:16 options:MTLResourceStorageModeShared];
        if (probe == nil || [probe contents] == NULL) {
            return;
        }
        aura_ready = 1;
        // The operations that move elements by index need a system whose
        // MPSGraph can broadcast and scatter along an axis.
        aura_indexed =
            [aura_graph_class instancesRespondToSelector:@selector(broadcastTensor:toShape:name:)] &&
            [aura_graph_class
                instancesRespondToSelector:@selector(scatterWithUpdatesTensor:indicesTensor:shape:axis:mode:name:)];
    }
}

int32_t aura_metal_available(void) {
    static pthread_once_t once = PTHREAD_ONCE_INIT;
    pthread_once(&once, aura_metal_load);
    return aura_ready;
}

// Whether a graph can hold the operations that move elements by index.
int32_t aura_metal_indexed(void) {
    return aura_metal_available() && aura_indexed;
}

void *aura_metal_buffer_new(size_t bytes) {
    if (!aura_metal_available() || bytes == 0) {
        return NULL;
    }
    @autoreleasepool {
        id<MTLBuffer> buffer = [aura_device newBufferWithLength:bytes options:MTLResourceStorageModeShared];
        if (buffer == nil || [buffer contents] == NULL) {
            return NULL;
        }
        return (__bridge_retained void *)buffer;
    }
}

uint8_t *aura_metal_buffer_contents(void *buffer) {
    if (buffer == NULL) {
        return NULL;
    }
    return (uint8_t *)[(__bridge id<MTLBuffer>)buffer contents];
}

void aura_metal_buffer_release(void *buffer) {
    if (buffer == NULL) {
        return;
    }
    @autoreleasepool {
        // The transfer gives the reference to a temporary, which releases it.
        (void)(__bridge_transfer id<MTLBuffer>)buffer;
    }
}

@interface AuraGraph : NSObject {
  @public
    MPSGraph *graph;
    NSMutableArray<MPSGraphTensor *> *tensors;
    // The wide words of the graph: each is a placeholder that every run
    // feeds from its own buffer.
    NSMutableDictionary<MPSGraphTensor *, MPSGraphTensorData *> *words;
}
@end

@implementation AuraGraph
@end

static MPSDataType aura_data_type(int32_t dtype) {
    switch (dtype) {
    case 0:
        return MPSDataTypeInt32;
    case 1:
        return MPSDataTypeInt64;
    case 2:
        return MPSDataTypeFloat32;
    case 4:
        return MPSDataTypeBool;
    default:
        return MPSDataTypeInvalid;
    }
}

static NSArray<NSNumber *> *aura_numbers(int32_t count, const int64_t *values) {
    NSMutableArray<NSNumber *> *numbers = [aura_array_class array];
    for (int32_t index = 0; index < count; index++) {
        [numbers addObject:[aura_number_class numberWithLongLong:values[index]]];
    }
    return numbers;
}

void *aura_metal_graph_new(void) {
    if (!aura_metal_available()) {
        return NULL;
    }
    @autoreleasepool {
        AuraGraph *made = [[AuraGraph alloc] init];
        made->graph = [[aura_graph_class alloc] init];
        made->tensors = [aura_array_class array];
        made->words = [aura_dictionary_class dictionary];
        if (made->graph == nil || made->tensors == nil || made->words == nil) {
            return NULL;
        }
        return (__bridge_retained void *)made;
    }
}

void aura_metal_graph_release(void *graph) {
    @autoreleasepool {
        (void)(__bridge_transfer AuraGraph *)graph;
    }
}

static int32_t aura_keep(AuraGraph *graph, MPSGraphTensor *tensor) {
    if (tensor == nil) {
        return -1;
    }
    [graph->tensors addObject:tensor];
    return (int32_t)[graph->tensors count] - 1;
}

int32_t aura_metal_graph_placeholder(void *handle, int32_t dtype, int32_t rank, const int64_t *shape) {
    @autoreleasepool {
        @try {
            AuraGraph *graph = (__bridge AuraGraph *)handle;
            MPSDataType type = aura_data_type(dtype);
            if (type == MPSDataTypeInvalid) {
                return -1;
            }
            return aura_keep(graph, [graph->graph placeholderWithShape:aura_numbers(rank, shape)
                                                             dataType:type
                                                                 name:nil]);
        } @catch (id thrown) {
            return -1;
        }
    }
}

static MPSGraphTensor *aura_binary(MPSGraph *g, int32_t code, MPSGraphTensor *a, MPSGraphTensor *b, BOOL real) {
    switch (code) {
    case 0:
        return [g additionWithPrimaryTensor:a secondaryTensor:b name:nil];
    case 1:
        return [g subtractionWithPrimaryTensor:a secondaryTensor:b name:nil];
    case 2:
        return [g multiplicationWithPrimaryTensor:a secondaryTensor:b name:nil];
    case 3:
        return [g divisionWithPrimaryTensor:a secondaryTensor:b name:nil];
    case 4:
        // Aura's minimum and maximum give a NaN when either operand is one.
        return real ? [g minimumWithNaNPropagationWithPrimaryTensor:a secondaryTensor:b name:nil]
                    : [g minimumWithPrimaryTensor:a secondaryTensor:b name:nil];
    case 5:
        return real ? [g maximumWithNaNPropagationWithPrimaryTensor:a secondaryTensor:b name:nil]
                    : [g maximumWithPrimaryTensor:a secondaryTensor:b name:nil];
    case 6:
        return [g powerWithPrimaryTensor:a secondaryTensor:b name:nil];
    default:
        return nil;
    }
}

static MPSGraphTensor *aura_unary(MPSGraph *g, int32_t code, MPSGraphTensor *a) {
    switch (code) {
    case 0:
        return [g absoluteWithTensor:a name:nil];
    case 1:
        return [g squareRootWithTensor:a name:nil];
    case 2:
        return [g exponentWithTensor:a name:nil];
    case 3:
        return [g logarithmWithTensor:a name:nil];
    case 6:
        return [g sinWithTensor:a name:nil];
    case 7:
        return [g cosWithTensor:a name:nil];
    case 8:
        return [g negativeWithTensor:a name:nil];
    case 65:
        return [g tanhWithTensor:a name:nil];
    case 66:
        return [g sigmoidWithTensor:a name:nil];
    case 67:
        return [g erfWithTensor:a name:nil];
    default:
        return nil;
    }
}

static MPSGraphTensor *aura_compare(MPSGraph *g, int32_t code, MPSGraphTensor *a, MPSGraphTensor *b) {
    switch (code) {
    case 0:
        return [g lessThanWithPrimaryTensor:a secondaryTensor:b name:nil];
    case 1:
        return [g lessThanOrEqualToWithPrimaryTensor:a secondaryTensor:b name:nil];
    case 2:
        return [g greaterThanWithPrimaryTensor:a secondaryTensor:b name:nil];
    case 3:
        return [g greaterThanOrEqualToWithPrimaryTensor:a secondaryTensor:b name:nil];
    case 4:
        return [g equalWithPrimaryTensor:a secondaryTensor:b name:nil];
    case 5:
        return [g notEqualWithPrimaryTensor:a secondaryTensor:b name:nil];
    default:
        return nil;
    }
}

static MPSGraphTensor *aura_reduce(MPSGraph *g, int32_t code, MPSGraphTensor *a, NSArray<NSNumber *> *axes) {
    switch (code) {
    case 0:
        return [g reductionSumWithTensor:a axes:axes name:nil];
    case 1:
        return [g reductionMinimumPropagateNaNWithTensor:a axes:axes name:nil];
    case 2:
        return [g reductionMaximumPropagateNaNWithTensor:a axes:axes name:nil];
    case 3:
        return [g meanOfTensor:a axes:axes name:nil];
    default:
        return nil;
    }
}

// The `int32` numbers `0, step, 2 * step, ...`, `count` of them.
static MPSGraphTensor *aura_steps(MPSGraph *g, int64_t count, int64_t step) {
    if (count < 1 || count > INT32_MAX || step < 0 || (count - 1) * step > INT32_MAX) {
        return nil;
    }
    int32_t *values = malloc((size_t)count * sizeof(int32_t));
    if (values == NULL) {
        return nil;
    }
    for (int64_t index = 0; index < count; index++) {
        values[index] = (int32_t)(index * step);
    }
    NSData *data = [aura_bytes_class dataWithBytesNoCopy:values length:(NSUInteger)count * sizeof(int32_t) freeWhenDone:YES];
    return [g constantWithData:data shape:@[[aura_number_class numberWithLongLong:count]] dataType:MPSDataTypeInt32];
}

// The places on an axis that a slice from `start` reads: `count` of them,
// `step` apart. `start` is a tensor of one `int64`.
static MPSGraphTensor *aura_positions(MPSGraph *g, MPSGraphTensor *start, int64_t count, int64_t step) {
    MPSGraphTensor *steps = aura_steps(g, count, step);
    if (steps == nil) {
        return nil;
    }
    MPSGraphTensor *first = [g castTensor:start toType:MPSDataTypeInt32 name:nil];
    return [g additionWithPrimaryTensor:steps secondaryTensor:first name:nil];
}

// A shape of `rank` ones with `middle` in place of the one at `axis`.
static NSArray<NSNumber *> *aura_around(NSUInteger rank, NSUInteger axis, NSArray<NSNumber *> *middle) {
    NSMutableArray<NSNumber *> *shape = [aura_array_class array];
    for (NSUInteger index = 0; index < axis; index++) {
        [shape addObject:[aura_number_class numberWithLongLong:1]];
    }
    [shape addObjectsFromArray:middle];
    while ([shape count] < rank) {
        [shape addObject:[aura_number_class numberWithLongLong:1]];
    }
    return shape;
}

// Indices as `int32`, and in `*valid` which of them name a place on an
// axis of `length` entries.
static MPSGraphTensor *aura_indices(MPSGraph *g, MPSGraphTensor *indices, int64_t length, MPSGraphTensor **valid) {
    MPSGraphTensor *at =
        indices.dataType == MPSDataTypeInt32 ? indices : [g castTensor:indices toType:MPSDataTypeInt32 name:nil];
    MPSGraphTensor *zero = [g constantWithScalar:0 dataType:MPSDataTypeInt32];
    MPSGraphTensor *end = [g constantWithScalar:(double)length dataType:MPSDataTypeInt32];
    *valid = [g logicalANDWithPrimaryTensor:[g greaterThanOrEqualToWithPrimaryTensor:at secondaryTensor:zero name:nil]
                            secondaryTensor:[g lessThanWithPrimaryTensor:at secondaryTensor:end name:nil]
                                       name:nil];
    // An index outside the axis becomes zero, and its element is masked.
    return [g selectWithPredicateTensor:*valid truePredicateTensor:at falsePredicateTensor:zero name:nil];
}

// For each index, the entry of `a` at that place along `axis`, which has
// `length` entries. An index outside the axis reads as zero.
static MPSGraphTensor *aura_take(MPSGraph *g, MPSGraphTensor *a, MPSGraphTensor *indices, NSUInteger axis,
                                 int64_t length) {
    MPSGraphTensor *valid = nil;
    MPSGraphTensor *at = aura_indices(g, indices, length, &valid);
    MPSGraphTensor *picked = [g gatherWithUpdatesTensor:a indicesTensor:at axis:axis batchDimensions:0 name:nil];
    NSArray<NSNumber *> *shape = picked.shape;
    MPSGraphTensor *mask = [g reshapeTensor:valid withShape:aura_around([shape count], axis, indices.shape) name:nil];
    mask = [g broadcastTensor:mask toShape:shape name:nil];
    return [g selectWithPredicateTensor:mask
                    truePredicateTensor:picked
                   falsePredicateTensor:[g constantWithScalar:0 shape:shape dataType:a.dataType]
                                   name:nil];
}

// Lanes of `size` entries along `axis`, where entry `k` is the sum of the
// entries of `a` whose index is `k`. An index outside the lane adds
// nothing.
static MPSGraphTensor *aura_index_add(MPSGraph *g, MPSGraphTensor *a, MPSGraphTensor *indices, NSUInteger axis,
                                      int64_t size, NSArray<NSNumber *> *result_shape) {
    int64_t count = 1;
    for (NSNumber *dimension in indices.shape) {
        count *= [dimension longLongValue];
    }
    NSArray<NSNumber *> *line = @[[aura_number_class numberWithLongLong:count]];
    // The axes the indices cover become one axis.
    NSMutableArray<NSNumber *> *flat = [aura_array_class arrayWithArray:result_shape];
    flat[axis] = line[0];
    MPSGraphTensor *valid = nil;
    MPSGraphTensor *at = aura_indices(g, [g reshapeTensor:indices withShape:line name:nil], size, &valid);
    MPSGraphTensor *mask = [g reshapeTensor:valid withShape:aura_around([flat count], axis, line) name:nil];
    mask = [g broadcastTensor:mask toShape:flat name:nil];
    MPSGraphTensor *kept = [g selectWithPredicateTensor:mask
                                    truePredicateTensor:[g reshapeTensor:a withShape:flat name:nil]
                                   falsePredicateTensor:[g constantWithScalar:0 shape:flat dataType:a.dataType]
                                                   name:nil];
    return [g scatterWithUpdatesTensor:kept
                         indicesTensor:at
                                 shape:result_shape
                                  axis:(NSInteger)axis
                                  mode:MPSGraphScatterModeAdd
                                  name:nil];
}

// The place along `axis`, which has `length` entries, of each lane's
// largest element, or smallest. The first one wins a tie, and a NaN wins
// over every number: the library's own arg reductions promise neither.
static MPSGraphTensor *aura_arg_reduce(MPSGraph *g, MPSGraphTensor *a, BOOL largest, NSUInteger axis, int64_t length,
                                       NSArray<NSNumber *> *result_shape) {
    NSArray<NSNumber *> *axes = @[[aura_number_class numberWithUnsignedLongLong:axis]];
    BOOL real = a.dataType == MPSDataTypeFloat32;
    MPSGraphTensor *best;
    if (largest) {
        best = real ? [g reductionMaximumPropagateNaNWithTensor:a axes:axes name:nil]
                    : [g reductionMaximumWithTensor:a axes:axes name:nil];
    } else {
        best = real ? [g reductionMinimumPropagateNaNWithTensor:a axes:axes name:nil]
                    : [g reductionMinimumWithTensor:a axes:axes name:nil];
    }
    NSArray<NSNumber *> *shape = a.shape;
    best = [g broadcastTensor:best toShape:shape name:nil];
    MPSGraphTensor *hit = [g equalWithPrimaryTensor:a secondaryTensor:best name:nil];
    if (real) {
        hit = [g selectWithPredicateTensor:[g isNaNWithTensor:best name:nil]
                       truePredicateTensor:[g isNaNWithTensor:a name:nil]
                      falsePredicateTensor:hit
                                      name:nil];
    }
    MPSGraphTensor *places = aura_steps(g, length, 1);
    if (places == nil) {
        return nil;
    }
    NSArray<NSNumber *> *line = @[[aura_number_class numberWithLongLong:length]];
    places = [g reshapeTensor:places withShape:aura_around([shape count], axis, line) name:nil];
    places = [g broadcastTensor:places toShape:shape name:nil];
    MPSGraphTensor *found = [g selectWithPredicateTensor:hit
                                     truePredicateTensor:places
                                    falsePredicateTensor:[g constantWithScalar:(double)length
                                                                         shape:shape
                                                                      dataType:MPSDataTypeInt32]
                                                    name:nil];
    found = [g reductionMinimumWithTensor:found axes:axes name:nil];
    return [g reshapeTensor:[g castTensor:found toType:MPSDataTypeInt64 name:nil] withShape:result_shape name:nil];
}

// One 64-bit word as a tensor of one `int64`. The library keeps an
// `int64` constant as a double, which rounds a word wider than 53 bits,
// so a wide word is a placeholder that each run feeds from a buffer.
static MPSGraphTensor *aura_word(AuraGraph *graph, uint64_t word) {
    MPSGraph *g = graph->graph;
    NSArray<NSNumber *> *shape = @[[aura_number_class numberWithLongLong:1]];
    if (word < (1ULL << 53)) {
        NSData *data = [aura_bytes_class dataWithBytes:&word length:sizeof(word)];
        return [g constantWithData:data shape:shape dataType:MPSDataTypeInt64];
    }
    id<MTLBuffer> buffer = [aura_device newBufferWithBytes:&word
                                                    length:sizeof(word)
                                                   options:MTLResourceStorageModeShared];
    if (buffer == nil) {
        return nil;
    }
    MPSGraphTensor *made = [g placeholderWithShape:shape dataType:MPSDataTypeInt64 name:nil];
    MPSGraphTensorData *data = [(MPSGraphTensorData *)[aura_data_class alloc] initWithMTLBuffer:buffer
                                                                                         shape:shape
                                                                                      dataType:MPSDataTypeInt64];
    [graph->words setObject:data forKey:made];
    return made;
}

// `bits` moved right by `count` places with zeros moved in, whichever
// way the library moves a signed word.
static MPSGraphTensor *aura_shift(AuraGraph *graph, MPSGraphTensor *bits, int count) {
    MPSGraph *g = graph->graph;
    MPSGraphTensor *moved = [g bitwiseRightShiftWithPrimaryTensor:bits secondaryTensor:aura_word(graph, (uint64_t)count) name:nil];
    return [g bitwiseANDWithPrimaryTensor:moved secondaryTensor:aura_word(graph, UINT64_MAX >> count) name:nil];
}

// The 64 random bits of each element of `index` in the stream `seed`
// names: `random_bits` of `tensor_kernels.rs`, in wrapping `int64`
// arithmetic.
static MPSGraphTensor *aura_random_bits(AuraGraph *graph, MPSGraphTensor *seed, MPSGraphTensor *index) {
    MPSGraph *g = graph->graph;
    MPSGraphTensor *golden = aura_word(graph, 0x9E3779B97F4A7C15ULL);
    MPSGraphTensor *bits = [g multiplicationWithPrimaryTensor:seed secondaryTensor:golden name:nil];
    bits = [g additionWithPrimaryTensor:bits
                        secondaryTensor:[g multiplicationWithPrimaryTensor:index
                                                           secondaryTensor:aura_word(graph, 0xD1B54A32D192ED03ULL)
                                                                      name:nil]
                                   name:nil];
    bits = [g additionWithPrimaryTensor:bits secondaryTensor:golden name:nil];
    bits = [g bitwiseXORWithPrimaryTensor:bits secondaryTensor:aura_shift(graph, bits, 30) name:nil];
    bits = [g multiplicationWithPrimaryTensor:bits secondaryTensor:aura_word(graph, 0xBF58476D1CE4E5B9ULL) name:nil];
    bits = [g bitwiseXORWithPrimaryTensor:bits secondaryTensor:aura_shift(graph, bits, 27) name:nil];
    bits = [g multiplicationWithPrimaryTensor:bits secondaryTensor:aura_word(graph, 0x94D049BB133111EBULL) name:nil];
    return [g bitwiseXORWithPrimaryTensor:bits secondaryTensor:aura_shift(graph, bits, 31) name:nil];
}

// A `float32` in `[0, 1)` from the top 24 bits of each word.
static MPSGraphTensor *aura_unit(AuraGraph *graph, MPSGraphTensor *bits) {
    MPSGraph *g = graph->graph;
    MPSGraphTensor *top = [g castTensor:aura_shift(graph, bits, 40) toType:MPSDataTypeFloat32 name:nil];
    return [g multiplicationWithPrimaryTensor:top
                              secondaryTensor:[g constantWithScalar:1.0 / 16777216.0 dataType:MPSDataTypeFloat32]
                                         name:nil];
}

// Random `float32` elements of `shape`: `filled` of `tensor_kernels.rs`.
// `fill` is 0 for uniform floats and 1 for normal ones.
static MPSGraphTensor *aura_random(AuraGraph *graph, int32_t fill, MPSGraphTensor *seed, NSArray<NSNumber *> *shape) {
    MPSGraph *g = graph->graph;
    if (![g respondsToSelector:@selector(bitwiseXORWithPrimaryTensor:secondaryTensor:name:)] ||
        ![g respondsToSelector:@selector(bitwiseANDWithPrimaryTensor:secondaryTensor:name:)] ||
        ![g respondsToSelector:@selector(bitwiseRightShiftWithPrimaryTensor:secondaryTensor:name:)] ||
        ![g respondsToSelector:@selector(coordinateAlongAxis:withShape:name:)]) {
        return nil;
    }
    int64_t count = 1;
    for (NSNumber *dimension in shape) {
        count *= [dimension longLongValue];
    }
    if (count < 1 || count > INT32_MAX || (fill != 0 && fill != 1)) {
        return nil;
    }
    NSNumber *elements = [aura_number_class numberWithLongLong:count];
    MPSGraphTensor *made;
    if (fill == 0) {
        MPSGraphTensor *index = [g coordinateAlongAxis:0 withShape:@[elements] name:nil];
        index = [g castTensor:index toType:MPSDataTypeInt64 name:nil];
        made = aura_unit(graph, aura_random_bits(graph, seed, index));
    } else {
        // The method of Box and Muller, over two uniform floats. Element
        // `j` reads the words at `2j` and `2j + 1`. The library computes
        // `index * 2 + 1` wrongly above 2^24 when only the final tensor
        // is an output, so both places are columns of one line of
        // coordinates and no arithmetic is done on a coordinate.
        if (count > INT32_MAX / 2) {
            return nil;
        }
        NSNumber *two = [aura_number_class numberWithLongLong:2];
        MPSGraphTensor *pairs = [g coordinateAlongAxis:0
                                             withShape:@[[aura_number_class numberWithLongLong:count * 2]]
                                                  name:nil];
        pairs = [g reshapeTensor:[g castTensor:pairs toType:MPSDataTypeInt64 name:nil] withShape:@[elements, two] name:nil];
        MPSGraphTensor *even = [g reshapeTensor:[g sliceTensor:pairs dimension:1 start:0 length:1 name:nil]
                                      withShape:@[elements]
                                           name:nil];
        MPSGraphTensor *odd = [g reshapeTensor:[g sliceTensor:pairs dimension:1 start:1 length:1 name:nil]
                                     withShape:@[elements]
                                          name:nil];
        MPSGraphTensor *first = aura_unit(graph, aura_random_bits(graph, seed, even));
        MPSGraphTensor *second = aura_unit(graph, aura_random_bits(graph, seed, odd));
        MPSGraphTensor *one = [g constantWithScalar:1.0 dataType:MPSDataTypeFloat32];
        MPSGraphTensor *spread = [g logarithmWithTensor:[g subtractionWithPrimaryTensor:one secondaryTensor:first name:nil]
                                                   name:nil];
        spread = [g squareRootWithTensor:[g multiplicationWithPrimaryTensor:spread
                                                            secondaryTensor:[g constantWithScalar:-2.0
                                                                                         dataType:MPSDataTypeFloat32]
                                                                       name:nil]
                                    name:nil];
        MPSGraphTensor *turn = [g multiplicationWithPrimaryTensor:second
                                                  secondaryTensor:[g constantWithScalar:(double)6.2831855f
                                                                               dataType:MPSDataTypeFloat32]
                                                             name:nil];
        made = [g multiplicationWithPrimaryTensor:spread secondaryTensor:[g cosWithTensor:turn name:nil] name:nil];
    }
    return [g reshapeTensor:made withShape:shape name:nil];
}

// Adds one step. `inputs` holds three tensor indices, with -1 for an
// operand the step does not have. `shape` is the step's result shape.
int32_t aura_metal_graph_step(void *handle, int32_t kind, int32_t code, const int32_t *inputs, int32_t dtype,
                              int32_t rank, const int64_t *shape, int32_t count, const int64_t *numbers) {
    @autoreleasepool {
        @try {
            AuraGraph *graph = (__bridge AuraGraph *)handle;
            MPSGraph *g = graph->graph;
            MPSGraphTensor *operands[3] = {nil, nil, nil};
            for (int index = 0; index < 3; index++) {
                if (inputs[index] >= 0) {
                    operands[index] = graph->tensors[(NSUInteger)inputs[index]];
                }
            }
            MPSGraphTensor *a = operands[0];
            MPSGraphTensor *b = operands[1];
            NSArray<NSNumber *> *result_shape = aura_numbers(rank, shape);
            MPSGraphTensor *made = nil;
            switch (kind) {
            case 0:
                made = aura_binary(g, code, a, b, a.dataType == MPSDataTypeFloat32);
                break;
            case 1:
                made = aura_unary(g, code, a);
                break;
            case 2:
                made = aura_compare(g, code, a, b);
                break;
            case 3:
                made = [g selectWithPredicateTensor:a
                                truePredicateTensor:b
                               falsePredicateTensor:operands[2]
                                               name:nil];
                break;
            case 4:
                made = aura_reduce(g, code, a, aura_numbers(count, numbers));
                if (made != nil) {
                    made = [g reshapeTensor:made withShape:result_shape name:nil];
                }
                break;
            case 5:
                made = [g matrixMultiplicationWithPrimaryTensor:a secondaryTensor:b name:nil];
                break;
            case 6:
                made = [g reshapeTensor:a withShape:result_shape name:nil];
                break;
            case 7:
                made = [g transposeTensor:a permutation:aura_numbers(count, numbers) name:nil];
                break;
            case 8:
                made = [g castTensor:a toType:aura_data_type(dtype) name:nil];
                break;
            case 9: {
                // A slice: `numbers` holds the axis, the step and the length.
                MPSGraphTensor *places = aura_positions(g, b, numbers[2], numbers[1]);
                if (places != nil) {
                    made = [g gatherWithUpdatesTensor:a
                                        indicesTensor:places
                                                 axis:(NSUInteger)numbers[0]
                                      batchDimensions:0
                                                 name:nil];
                }
                break;
            }
            case 10: {
                // The reverse of a slice: `numbers` holds the axis, the
                // step and the length of the operand's axis.
                MPSGraphTensor *places = aura_positions(g, b, numbers[2], numbers[1]);
                if (places != nil) {
                    made = [g scatterWithUpdatesTensor:a
                                         indicesTensor:places
                                                 shape:result_shape
                                                  axis:(NSInteger)numbers[0]
                                                  mode:MPSGraphScatterModeAdd
                                                  name:nil];
                }
                break;
            }
            case 11:
                made = [g broadcastTensor:a toShape:result_shape name:nil];
                break;
            case 12:
                made = [g concatTensor:a withTensor:b dimension:(NSInteger)numbers[0] name:nil];
                break;
            case 13:
                made = aura_take(g, a, b, (NSUInteger)numbers[0], numbers[1]);
                break;
            case 14:
                made = aura_index_add(g, a, b, (NSUInteger)numbers[0], numbers[1], result_shape);
                break;
            case 15:
                made = aura_arg_reduce(g, a, code == 1, (NSUInteger)numbers[0], numbers[1], result_shape);
                break;
            case 16:
                made = aura_random(graph, code, a, result_shape);
                break;
            default:
                break;
            }
            // A step that changes nothing can give its operand back. Each
            // step has its own tensor, so that a run can write each one.
            if (made != nil && (made == a || made == b || made == operands[2])) {
                made = [g identityWithTensor:made name:nil];
            }
            return aura_keep(graph, made);
        } @catch (id thrown) {
            return -1;
        }
    }
}

// Runs the graph with each placeholder reading its buffer and each target
// written into its output. Returns 0 when it ran.
int32_t aura_metal_graph_run(void *handle, int32_t feeds, const int32_t *placeholders, void *const *buffers,
                             int32_t count, const int32_t *targets, void *const *outputs) {
    @autoreleasepool {
        @try {
            AuraGraph *graph = (__bridge AuraGraph *)handle;
            NSMutableDictionary<MPSGraphTensor *, MPSGraphTensorData *> *feed = [aura_dictionary_class dictionary];
            for (int32_t index = 0; index < feeds; index++) {
                MPSGraphTensor *tensor = graph->tensors[(NSUInteger)placeholders[index]];
                MPSGraphTensorData *data =
                    [(MPSGraphTensorData *)[aura_data_class alloc] initWithMTLBuffer:(__bridge id<MTLBuffer>)buffers[index]
                                                                              shape:tensor.shape
                                                                           dataType:tensor.dataType];
                if (data == nil) {
                    return -1;
                }
                [feed setObject:data forKey:tensor];
            }
            [feed addEntriesFromDictionary:graph->words];
            NSMutableDictionary<MPSGraphTensor *, MPSGraphTensorData *> *results = [aura_dictionary_class dictionary];
            for (int32_t index = 0; index < count; index++) {
                MPSGraphTensor *result = graph->tensors[(NSUInteger)targets[index]];
                if ([results objectForKey:result] != nil) {
                    // Copied below from the output that names the same tensor.
                    continue;
                }
                MPSGraphTensorData *written =
                    [(MPSGraphTensorData *)[aura_data_class alloc] initWithMTLBuffer:(__bridge id<MTLBuffer>)outputs[index]
                                                                              shape:result.shape
                                                                           dataType:result.dataType];
                if (written == nil) {
                    return -1;
                }
                [results setObject:written forKey:result];
            }
            [graph->graph runWithMTLCommandQueue:aura_queue feeds:feed targetOperations:nil resultsDictionary:results];
            for (int32_t index = 1; index < count; index++) {
                for (int32_t first = 0; first < index; first++) {
                    if (targets[first] == targets[index] ||
                        graph->tensors[(NSUInteger)targets[first]] == graph->tensors[(NSUInteger)targets[index]]) {
                        id<MTLBuffer> from = (__bridge id<MTLBuffer>)outputs[first];
                        id<MTLBuffer> to = (__bridge id<MTLBuffer>)outputs[index];
                        NSUInteger bytes = [from length] < [to length] ? [from length] : [to length];
                        memcpy([to contents], [from contents], bytes);
                        break;
                    }
                }
            }
            return 0;
        } @catch (id thrown) {
            return -1;
        }
    }
}
