@interface Widget : NSObject
- (int)run:(int)x;
@end

@implementation Widget
- (int)run:(int)x {
    return x + 1;
}
- (void)start {
    [self run:1];
}
@end
