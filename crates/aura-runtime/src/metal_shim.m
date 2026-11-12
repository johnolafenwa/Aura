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

static id<MTLDevice> aura_device;
static id<MTLCommandQueue> aura_queue;
static Class aura_graph_class;
static Class aura_data_class;
static Class aura_number_class;
static Class aura_array_class;
static Class aura_dictionary_class;
static int aura_ready;

typedef id<MTLDevice> (*AuraCreateDevice)(void);

static void aura_metal_load(void) {
    @autoreleasepool {
        void *metal = dlopen("/System/Library/Frameworks/Metal.framework/Metal", RTLD_LAZY | RTLD_GLOBAL);
        void *graph = dlopen(
            "/System/Library/Frameworks/MetalPerformanceShadersGraph.framework/MetalPerformanceShadersGraph",
            RTLD_LAZY | RTLD_GLOBAL);
        if (metal == NULL || graph == NULL) {
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
        if (aura_queue == nil || aura_graph_class == Nil || aura_data_class == Nil ||
            aura_number_class == Nil || aura_array_class == Nil || aura_dictionary_class == Nil) {
            return;
        }
        // A virtual machine can name a device whose allocations fail.
        id<MTLBuffer> probe = [aura_device newBufferWithLength:16 options:MTLResourceStorageModeShared];
        if (probe == nil || [probe contents] == NULL) {
            return;
        }
        aura_ready = 1;
    }
}

int32_t aura_metal_available(void) {
    static pthread_once_t once = PTHREAD_ONCE_INIT;
    pthread_once(&once, aura_metal_load);
    return aura_ready;
}

void *aura_metal_buffer_new(size_t bytes) {
    if (!aura_metal_available() || bytes == 0) {
        return NULL;
    }
    @autoreleasepool {
        id<MTLBuffer> buffer = [aura_device newBufferWithLength:bytes options:MTLResourceStorageModeShared];
        return (__bridge_retained void *)buffer;
    }
}

uint8_t *aura_metal_buffer_contents(void *buffer) {
    return (uint8_t *)[(__bridge id<MTLBuffer>)buffer contents];
}

void aura_metal_buffer_release(void *buffer) {
    @autoreleasepool {
        // The transfer gives the reference to a temporary, which releases it.
        (void)(__bridge_transfer id<MTLBuffer>)buffer;
    }
}

@interface AuraGraph : NSObject {
  @public
    MPSGraph *graph;
    NSMutableArray<MPSGraphTensor *> *tensors;
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
        if (made->graph == nil || made->tensors == nil) {
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
            default:
                break;
            }
            return aura_keep(graph, made);
        } @catch (id thrown) {
            return -1;
        }
    }
}

// Runs the graph with each placeholder reading its buffer and the target
// written into `output`. Returns 0 when it ran.
int32_t aura_metal_graph_run(void *handle, int32_t feeds, const int32_t *placeholders, void *const *buffers,
                             int32_t target, void *output) {
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
                [feed setObject:data forKey:tensor];
            }
            MPSGraphTensor *result = graph->tensors[(NSUInteger)target];
            MPSGraphTensorData *written =
                [(MPSGraphTensorData *)[aura_data_class alloc] initWithMTLBuffer:(__bridge id<MTLBuffer>)output
                                                                          shape:result.shape
                                                                       dataType:result.dataType];
            NSMutableDictionary<MPSGraphTensor *, MPSGraphTensorData *> *results = [aura_dictionary_class dictionary];
            [results setObject:written forKey:result];
            [graph->graph runWithMTLCommandQueue:aura_queue feeds:feed targetOperations:nil resultsDictionary:results];
            return 0;
        } @catch (id thrown) {
            return -1;
        }
    }
}
